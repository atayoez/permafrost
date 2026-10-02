//! Downloads community blocklists and keeps them fresh.
//!
//! Lists live in `<state dir>/community/<id>.txt`, one hostname per line, so a
//! freeze keeps working offline with the last good copy.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use permafrost_common::community::{self, Source};

const REFRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RETRY: Duration = Duration::from_secs(60 * 60);
const MAX_DOWNLOAD: u64 = 32 * 1024 * 1024;
/// A real list has thousands of entries; fewer means a broken download.
const MIN_ENTRIES: usize = 100;

type Download = (String, Result<Vec<String>, String>);

pub struct Community {
    dir: PathBuf,
    lists: HashMap<String, Vec<String>>,
    attempted: HashMap<String, Instant>,
    /// Bumped whenever a list changes, so the hosts file gets rewritten.
    generation: u64,
    tx: mpsc::Sender<Download>,
    rx: mpsc::Receiver<Download>,
}

impl Community {
    pub fn new(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut lists = HashMap::new();
        for source in community::SOURCES {
            if let Ok(text) = std::fs::read_to_string(dir.join(format!("{}.txt", source.id))) {
                lists.insert(source.id.to_owned(), text.lines().map(str::to_owned).collect());
            }
        }
        Self { dir, lists, attempted: HashMap::new(), generation: 0, tx, rx }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn hosts(&self, id: &str) -> &[String] {
        self.lists.get(id).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn sizes(&self) -> BTreeMap<String, usize> {
        self.lists.iter().map(|(id, hosts)| (id.clone(), hosts.len())).collect()
    }

    /// Takes in finished downloads and starts any that are due.
    /// Returns whether a list changed.
    pub fn refresh(&mut self, wanted: &BTreeSet<String>) -> bool {
        let mut changed = false;
        while let Ok((id, result)) = self.rx.try_recv() {
            match result {
                Ok(hosts) => {
                    tracing::info!("updated community list {id}: {} sites", hosts.len());
                    if let Err(e) = self.store(&id, &hosts) {
                        tracing::warn!("couldn't save community list {id}: {e}");
                    }
                    self.lists.insert(id, hosts);
                    self.generation += 1;
                    changed = true;
                }
                Err(e) => tracing::warn!("couldn't download community list {id}: {e}"),
            }
        }

        for id in wanted {
            let Some(source) = community::find(id) else { continue };
            let recently_tried = self.attempted.get(id).is_some_and(|t| t.elapsed() < RETRY);
            if recently_tried || (self.lists.contains_key(id) && !self.is_stale(id)) {
                continue;
            }
            self.attempted.insert(id.clone(), Instant::now());
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send((source.id.to_owned(), download(source)));
            });
        }
        changed
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.txt"))
    }

    fn is_stale(&self, id: &str) -> bool {
        std::fs::metadata(self.path(id))
            .and_then(|m| m.modified())
            .map(|modified| SystemTime::now().duration_since(modified).unwrap_or_default() > REFRESH)
            .unwrap_or(true)
    }

    fn store(&self, id: &str, hosts: &[String]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.path(id).with_extension("tmp");
        std::fs::write(&tmp, hosts.join("\n"))?;
        std::fs::rename(tmp, self.path(id))
    }
}

fn download(source: &Source) -> Result<Vec<String>, String> {
    let text = ureq::get(source.url)
        .header("User-Agent", concat!("permafrostd/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| e.to_string())?
        .into_body()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let hosts = community::parse_hosts(&text);
    if hosts.len() < MIN_ENTRIES {
        return Err(format!("only {} entries, keeping the previous copy", hosts.len()));
    }
    Ok(hosts)
}
