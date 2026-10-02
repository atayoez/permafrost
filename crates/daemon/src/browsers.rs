//! Turns off DNS-over-HTTPS in browsers while something is blocked.
//!
//! Encrypted DNS goes straight to a resolver and skips `/etc/hosts`, so
//! browsers using it would ignore every block. Browsers read these policies
//! when they start.

use std::path::Path;

const CHROMIUM: &str = "{\n  \"DnsOverHttpsMode\": \"off\"\n}\n";
const FIREFOX: &str = "{\n  \"policies\": {\n    \"DNSOverHTTPS\": {\n      \"Enabled\": false,\n      \"Locked\": true\n    }\n  }\n}\n";

struct Policy {
    file: &'static str,
    /// Where the browser is installed; the policy is only written if one exists.
    installed: &'static [&'static str],
    contents: &'static str,
}

const POLICIES: &[Policy] = &[
    Policy {
        file: "/etc/opt/chrome/policies/managed/permafrost.json",
        installed: &["/opt/google/chrome"],
        contents: CHROMIUM,
    },
    Policy {
        file: "/etc/chromium/policies/managed/permafrost.json",
        installed: &["/usr/lib64/chromium-browser", "/usr/lib/chromium", "/usr/lib/chromium-browser"],
        contents: CHROMIUM,
    },
    Policy {
        file: "/etc/brave/policies/managed/permafrost.json",
        installed: &["/opt/brave.com/brave"],
        contents: CHROMIUM,
    },
    Policy {
        file: "/etc/opt/edge/policies/managed/permafrost.json",
        installed: &["/opt/microsoft/msedge"],
        contents: CHROMIUM,
    },
    // Firefox reads a single shared file, so only one we created gets touched.
    Policy {
        file: "/etc/firefox/policies/policies.json",
        installed: &["/usr/lib64/firefox", "/usr/lib/firefox"],
        contents: FIREFOX,
    },
];

/// Writes (`on`) or removes the policies. Never touches a file someone else wrote.
pub fn set_doh_blocked(on: bool, dry_run: bool) {
    for policy in POLICIES {
        let path = Path::new(policy.file);
        let existing = std::fs::read_to_string(path).ok();
        let ours = existing.as_deref() == Some(policy.contents);
        if on {
            let installed = policy.installed.iter().any(|p| Path::new(p).exists());
            if !installed || ours {
                continue;
            }
            if existing.is_some() {
                tracing::warn!("{} has other policies; not turning off DNS-over-HTTPS there", policy.file);
                continue;
            }
            tracing::info!("turning off DNS-over-HTTPS via {}", policy.file);
            if !dry_run && let Err(e) = write(path, policy.contents) {
                tracing::warn!("couldn't write {}: {e}", policy.file);
            }
        } else if ours {
            tracing::info!("restoring DNS-over-HTTPS via {}", policy.file);
            if !dry_run && let Err(e) = std::fs::remove_file(path) {
                tracing::warn!("couldn't remove {}: {e}", policy.file);
            }
        }
    }
}

fn write(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)
}
