//! Finding and closing blocked apps.
//!
//! GNOME starts every app in its own systemd scope (`app-gnome-<id>-<n>.scope`,
//! `app-flatpak-<id>-<n>.scope`), so the cgroup tree tells us which app a
//! process belongs to. Apps started some other way are matched by executable.

use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

const CGROUP_ROOT: &str = "/sys/fs/cgroup/user.slice";

/// Executables that launch other programs, so they can't identify an app.
const GENERIC_EXECUTABLES: &[&str] = &["flatpak", "env", "sh", "bash", "python3", "gjs", "gapplication", "snap"];

/// The app ID inside a systemd scope name, if it is an app scope.
pub fn app_id_from_scope(name: &str) -> Option<String> {
    let rest = name.strip_prefix("app-")?.strip_suffix(".scope")?;
    let (rest, _random) = rest.rsplit_once('-')?;
    // Dashes in the app ID are escaped, so a remaining dash ends the launcher.
    let id = rest.split_once('-').map_or(rest, |(_launcher, id)| id);
    Some(unescape(id)).filter(|id| !id.is_empty())
}

/// Reverses systemd's `\xNN` escaping.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("\\x") {
        out.push_str(&rest[..i]);
        let hex = rest.get(i + 2..i + 4).and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte as char);
                rest = &rest[i + 4..];
            }
            None => {
                out.push_str("\\x");
                rest = &rest[i + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(Default)]
pub struct AppEnforcer {
    executables: HashMap<String, Option<String>>,
}

impl AppEnforcer {
    /// Closes running blocked apps. Returns the IDs of the apps it closed.
    pub fn enforce(&mut self, blocked: &BTreeSet<String>, dry_run: bool) -> Vec<String> {
        if blocked.is_empty() {
            return Vec::new();
        }
        let mut closed = BTreeSet::new();

        for scope in app_scopes(Path::new(CGROUP_ROOT), 0) {
            let Some(id) = scope.file_name().and_then(|n| n.to_str()).and_then(app_id_from_scope) else {
                continue;
            };
            if blocked.contains(&id) && has_processes(&scope) {
                tracing::info!("closing {id} ({})", scope.display());
                if !dry_run && let Err(e) = std::fs::write(scope.join("cgroup.kill"), "1") {
                    tracing::warn!("couldn't close {id}: {e}");
                    continue;
                }
                closed.insert(id);
            }
        }

        let by_exe: HashMap<String, &String> = blocked
            .iter()
            .filter_map(|id| self.executable(id).map(|exe| (exe, id)))
            .collect();
        if !by_exe.is_empty() {
            for (pid, exe) in user_processes() {
                if let Some(id) = by_exe.get(&exe) {
                    tracing::info!("closing {id} (pid {pid})");
                    if !dry_run {
                        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
                    }
                    closed.insert((*id).clone());
                }
            }
        }
        closed.into_iter().collect()
    }

    /// Which of `apps` are running now.
    pub fn running(&mut self, apps: &BTreeSet<String>) -> BTreeSet<String> {
        if apps.is_empty() {
            return BTreeSet::new();
        }
        let mut running: BTreeSet<String> = app_scopes(Path::new(CGROUP_ROOT), 0)
            .iter()
            .filter(|scope| has_processes(scope))
            .filter_map(|scope| scope.file_name()?.to_str().and_then(app_id_from_scope))
            .filter(|id| apps.contains(id))
            .collect();
        let by_exe: HashMap<String, &String> =
            apps.iter().filter_map(|id| self.executable(id).map(|exe| (exe, id))).collect();
        if !by_exe.is_empty() {
            for (_, exe) in user_processes() {
                if let Some(id) = by_exe.get(&exe) {
                    running.insert((*id).clone());
                }
            }
        }
        running
    }

    fn executable(&mut self, app_id: &str) -> Option<String> {
        self.executables.entry(app_id.to_owned()).or_insert_with(|| desktop_executable(app_id)).clone()
    }
}

fn app_scopes(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("app-") && name.ends_with(".scope") {
            found.push(path);
        } else if depth < 6 && (name.ends_with(".slice") || name.ends_with(".service")) {
            found.extend(app_scopes(&path, depth + 1));
        }
    }
    found
}

fn has_processes(scope: &Path) -> bool {
    std::fs::read_to_string(scope.join("cgroup.procs")).is_ok_and(|p| !p.trim().is_empty())
}

/// Processes owned by regular users, with their executable's file name.
fn user_processes() -> Vec<(i32, String)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            if entry.metadata().ok()?.uid() < 1000 {
                return None;
            }
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            Some((pid, exe.file_name()?.to_str()?.to_owned()))
        })
        .collect()
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/applications",
        "/usr/local/share/applications",
        "/var/lib/flatpak/exports/share/applications",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    if let Ok(homes) = std::fs::read_dir("/home") {
        for home in homes.flatten() {
            dirs.push(home.path().join(".local/share/applications"));
            dirs.push(home.path().join(".local/share/flatpak/exports/share/applications"));
        }
    }
    dirs
}

/// The executable an app's desktop file starts, unless it's a launcher like `flatpak`.
fn desktop_executable(app_id: &str) -> Option<String> {
    let file = format!("{app_id}.desktop");
    let contents = application_dirs().into_iter().find_map(|dir| std::fs::read_to_string(dir.join(&file)).ok())?;
    exec_name(&contents)
}

fn exec_name(desktop_file: &str) -> Option<String> {
    let mut in_entry = false;
    for line in desktop_file.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(exec) = line.strip_prefix("Exec=") {
            let program = exec
                .split_whitespace()
                .find(|t| *t != "env" && !t.contains('='))?
                .trim_matches('"');
            let name = Path::new(program).file_name()?.to_str()?.to_owned();
            return (!GENERIC_EXECUTABLES.contains(&name.as_str())).then_some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scope_names() {
        let cases = [
            ("app-flatpak-com.valvesoftware.Steam-12345.scope", "com.valvesoftware.Steam"),
            ("app-gnome-org.gnome.Calculator-4242.scope", "org.gnome.Calculator"),
            ("app-gnome-steam-777.scope", "steam"),
            ("app-gnome-org.gnome.Ptyxis\\x2dNightly-1.scope", "org.gnome.Ptyxis-Nightly"),
            ("app-org.example.NoLauncher-9.scope", "org.example.NoLauncher"),
        ];
        for (scope, id) in cases {
            assert_eq!(app_id_from_scope(scope).as_deref(), Some(id), "{scope}");
        }
        assert_eq!(app_id_from_scope("session-2.scope"), None);
        assert_eq!(app_id_from_scope("app-gnome-foo.service"), None);
    }

    #[test]
    fn reads_exec_lines() {
        let steam = "[Desktop Entry]\nName=Steam\nExec=/usr/bin/steam %U\n[Desktop Action Store]\nExec=steam steam://store\n";
        assert_eq!(exec_name(steam).as_deref(), Some("steam"));
        let with_env = "[Desktop Entry]\nExec=env GDK_BACKEND=x11 /opt/app/bin/thing --flag\n";
        assert_eq!(exec_name(with_env).as_deref(), Some("thing"));
        let flatpak = "[Desktop Entry]\nExec=/usr/bin/flatpak run --branch=stable com.discordapp.Discord\n";
        assert_eq!(exec_name(flatpak), None);
    }
}
