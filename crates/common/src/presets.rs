//! Ready-made block lists people can add with one click.

use crate::model::BlockList;

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub sites: &'static [&'static str],
    pub apps: &'static [&'static str],
    pub safe_search: bool,
}

impl Preset {
    pub fn to_list(&self, id: String) -> BlockList {
        BlockList {
            id,
            name: self.name.to_owned(),
            sites: self.sites.iter().map(|s| (*s).to_owned()).collect(),
            apps: self.apps.iter().map(|a| (*a).to_owned()).collect(),
            safe_search: self.safe_search,
        }
    }

    /// Adds this preset's entries to an existing list, keeping what's there.
    pub fn merge_into(&self, list: &mut BlockList) {
        for site in self.sites {
            if !list.sites.iter().any(|s| s == site) {
                list.sites.push((*site).to_owned());
            }
        }
        for app in self.apps {
            if !list.apps.iter().any(|a| a == app) {
                list.apps.push((*app).to_owned());
            }
        }
        list.safe_search |= self.safe_search;
    }
}

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "social",
        name: "Social Media",
        description: "Facebook, Instagram, X, TikTok, Reddit and more",
        sites: &[
            "facebook.com", "instagram.com", "x.com", "twitter.com", "tiktok.com", "reddit.com",
            "snapchat.com", "linkedin.com", "pinterest.com", "threads.net", "threads.com",
            "bsky.app", "tumblr.com", "mastodon.social",
        ],
        apps: &[],
        safe_search: false,
    },
    Preset {
        id: "video",
        name: "Video",
        description: "YouTube, Netflix, Twitch and other streaming sites",
        sites: &[
            "youtube.com", "youtu.be", "netflix.com", "twitch.tv", "primevideo.com",
            "disneyplus.com", "hulu.com", "max.com", "vimeo.com", "dailymotion.com", "kick.com",
        ],
        apps: &[],
        safe_search: false,
    },
    Preset {
        id: "news",
        name: "News",
        description: "Major news sites and aggregators",
        sites: &[
            "news.google.com", "cnn.com", "bbc.com", "bbc.co.uk", "nytimes.com", "theguardian.com",
            "foxnews.com", "reuters.com", "washingtonpost.com", "news.ycombinator.com", "apnews.com",
        ],
        apps: &[],
        safe_search: false,
    },
    Preset {
        id: "shopping",
        name: "Shopping",
        description: "Amazon, eBay, AliExpress, Temu and more",
        sites: &[
            "amazon.com", "ebay.com", "aliexpress.com", "temu.com", "etsy.com", "walmart.com",
            "shein.com", "wish.com",
        ],
        apps: &[],
        safe_search: false,
    },
    Preset {
        id: "games",
        name: "Games",
        description: "Game stores, browser games and game launchers",
        sites: &[
            "store.steampowered.com", "steamcommunity.com", "epicgames.com", "roblox.com",
            "chess.com", "lichess.org", "poki.com", "crazygames.com", "miniclip.com",
        ],
        apps: &[
            "com.valvesoftware.Steam", "steam", "com.heroicgameslauncher.hgl", "net.lutris.Lutris",
            "org.prismlauncher.PrismLauncher", "com.usebottles.bottles",
        ],
        safe_search: false,
    },
    Preset {
        id: "gambling",
        name: "Gambling",
        description: "Sports betting, online casinos, poker and crypto gambling sites",
        sites: &[
            "bet365.com", "betfair.com", "williamhill.com", "paddypower.com", "ladbrokes.com",
            "coral.co.uk", "skybet.com", "betway.com", "unibet.com", "bwin.com", "888.com",
            "888casino.com", "888poker.com", "pokerstars.com", "ggpoker.com", "partypoker.com",
            "draftkings.com", "fanduel.com", "betmgm.com", "pointsbet.com", "bovada.lv",
            "betonline.ag", "mybookie.ag", "pinnacle.com", "betsson.com", "leovegas.com",
            "casumo.com", "mrgreen.com", "1xbet.com", "melbet.com", "mostbet.com", "22bet.com",
            "stake.com", "stake.us", "roobet.com", "bc.game", "rollbit.com", "chumbacasino.com",
            "pulsz.com", "nesine.com", "bilyoner.com", "misli.com", "iddaa.com", "tuttur.com",
        ],
        apps: &[],
        safe_search: false,
    },
    Preset {
        id: "adult",
        name: "Adult Content",
        description: "Major adult sites, and forces SafeSearch on Google, Bing, DuckDuckGo and YouTube",
        sites: &[
            "pornhub.com", "xvideos.com", "xnxx.com", "xhamster.com", "xhamsterlive.com",
            "redtube.com", "youporn.com", "tube8.com", "spankbang.com", "eporner.com", "beeg.com",
            "porn.com", "txxx.com", "hqporner.com", "motherless.com", "thisvid.com", "erome.com",
            "brazzers.com", "realitykings.com", "bangbros.com", "onlyfans.com", "fansly.com",
            "chaturbate.com", "stripchat.com", "livejasmin.com", "bongacams.com", "cam4.com",
            "camsoda.com", "myfreecams.com", "rule34.xxx", "rule34.paheal.net", "e-hentai.org",
            "nhentai.net", "hanime.tv", "literotica.com",
        ],
        apps: &[],
        safe_search: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_sites_are_normalized() {
        for preset in PRESETS {
            for site in preset.sites {
                assert_eq!(crate::domain::normalize(site).as_deref(), Some(*site), "{} in {}", site, preset.id);
            }
        }
    }

    #[test]
    fn merge_keeps_existing_entries() {
        let mut list = BlockList { sites: vec!["example.org".into(), "reddit.com".into()], ..Default::default() };
        find("social").unwrap().merge_into(&mut list);
        assert_eq!(list.sites.iter().filter(|s| *s == "reddit.com").count(), 1);
        assert!(list.sites.contains(&"example.org".to_owned()));
    }
}
