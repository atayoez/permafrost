<div align="center">

<img src="data/icons/hicolor/scalable/apps/io.github.atayoez.Permafrost.svg" width="128" alt="Permafrost icon: a padlock frozen under snow">

# Permafrost

**Block distracting websites and apps until the timer runs out.**

A focus app for the GNOME desktop, in the spirit of Cold Turkey. Lock a freeze and it can't be stopped early — not by closing the app, not by restarting your computer.

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
![GNOME 50+](https://img.shields.io/badge/GNOME-50%2B-4a86cf)
![libadwaita 1.9](https://img.shields.io/badge/libadwaita-1.9-4a86cf)
![Rust](https://img.shields.io/badge/Rust-2024-orange)

<img src="docs/screenshots/freeze.png" width="800" alt="The Freeze page: duration picker, breaks, the Freeze Now button and the filters to block">

</div>

## Features

- **Freeze now** for 25 minutes, 1, 2 or 4 hours, or any custom time.
- **Locks that hold.** A locked freeze can't be stopped early, only extended. Filters in a locked freeze can only get stricter: you can add sites, never remove them.
- **Filters** for social media, video, news, shopping, games, gambling, adult content and harmful sites, plus your own.
- **Community lists** add hundreds of thousands of sites to a filter, from [StevenBlack/hosts](https://github.com/StevenBlack/hosts), [HaGeZi](https://github.com/hagezi/dns-blocklists) and [The Block List Project](https://github.com/blocklistproject/Lists).
- **App blocking.** Blocked apps, Flatpaks included, are closed as soon as they open.
- **Schedules** block filters automatically, at set hours or all day, on the days you choose.
- **Breaks.** Pomodoro-style cycles (25 + 5, 50 + 10, 90 + 15) lift the blocks for a short break, while the freeze itself stays locked.
- **Force SafeSearch** on Google, Bing, DuckDuckGo and YouTube.
- **History** of your focus time, day by day, with a streak.
- **Hard to get around.** Blocks survive restarts and stopping the service, changing the system clock doesn't end a freeze, and browsers are kept from bypassing blocks through encrypted DNS.

## Screenshots

| | |
|---|---|
| ![A locked freeze in dark style, counting down with the next break coming up](docs/screenshots/frozen.png) | ![The Filters page listing the built-in and custom filters](docs/screenshots/filters.png) |
| ![The Gambling filter with its community lists and websites](docs/screenshots/filter.png) | ![Schedules: weekday work hours and an all-day schedule](docs/screenshots/schedules.png) |
| ![History: focus time today, this week and the day streak](docs/screenshots/history.png) | ![Settings: lock by default, SafeSearch and restore defaults](docs/screenshots/settings.png) |

## Installation

Permafrost needs a system service to edit `/etc/hosts` and close apps, so it's installed system-wide rather than as a Flatpak.

**Requirements:** a Linux desktop with GNOME 50 or newer (libadwaita 1.9, GTK 4.22), systemd and Rust 1.92 or newer. On Fedora 44:

```sh
sudo dnf install gtk4-devel libadwaita-devel blueprint-compiler glib2-devel
```

Then build and install. The script builds as you and only asks for your password to install:

```sh
git clone https://github.com/atayoez/permafrost.git
cd permafrost
./install.sh
```

This installs the app, the `permafrostd` service, its D-Bus policy, the desktop file and icons, and starts the service. Open **Permafrost** from your apps.

To update, pull and run `./install.sh` again. To remove it:

```sh
./install.sh --uninstall
```

Your filters, schedules and history stay in `/var/lib/permafrost` until you delete that folder.

## How it works

```
┌─────────────── your session ───────────────┐
│  Permafrost app (GTK 4 + libadwaita, Rust) │
└────────────────────┬───────────────────────┘
                     │ D-Bus system bus: io.github.atayoez.Permafrost1
┌────────────────────┴───────────────────────┐
│  permafrostd (systemd service, root)       │
│   ├─ /etc/hosts      blocks websites       │
│   ├─ app scopes      closes blocked apps   │
│   ├─ browser policy  turns off DoH         │
│   └─ /var/lib/permafrost  state + lists    │
└────────────────────────────────────────────┘
```

- **The service owns every rule.** The app never runs as root; it only asks `permafrostd` for changes, and the service refuses anything that would loosen a lock. Closing or killing the app changes nothing.
- **Websites** are blocked through a managed section of `/etc/hosts`, which every browser and app respects. Common subdomains (`www.`, `m.`, …) are blocked along with each site. Large community lists are written as IPv4-only lines to keep the file small.
- **Apps** are found through the systemd scope GNOME starts every app in (`app-gnome-…scope`, `app-flatpak-…scope`) and closed with `cgroup.kill`. Apps started some other way are matched by their executable.
- **Encrypted DNS** (DNS-over-HTTPS) would skip `/etc/hosts`, so while anything is blocked the service installs managed policies that turn it off in Firefox, Chrome, Chromium, Brave and Edge, and removes them afterwards. It never touches a policy file it didn't create. Browsers pick the policy up when they start.
- **Clock changes** don't end freezes early: the service measures elapsed time with the boot clock and shifts the end time if the wall clock jumps.
- **Community lists** are downloaded each time the service starts and when one is added, and the last good copy is kept for offline use.

> [!NOTE]
> Permafrost aims for "just enough friction", not a fortress. Anyone with root access can undo it. Its job is to make breaking a freeze slow and deliberate, not impossible.

## Development

The workspace has three crates:

| Crate | What it is |
|---|---|
| `crates/common` | The model, presets, community list sources and the D-Bus proxy |
| `crates/daemon` | `permafrostd`, the system service |
| `crates/app` | The GTK app; its UI is in `crates/app/data/ui/*.blp` ([Blueprint](https://gnome.pages.gitlab.gnome.org/blueprint-compiler/)) |

Run the tests and lints:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
```

Try everything without root or touching your system: run the service on the session bus in dry-run mode, and point the app at it.

```sh
cargo run -p permafrostd -- --session --dry-run --state-dir /tmp/permafrost --hosts /tmp/permafrost/hosts
PERMAFROST_BUS=session cargo run -p permafrost
```

`--dry-run` logs what would be blocked instead of writing `/etc/hosts`, browser policies or closing apps.

Debug builds can render the window to a PNG and quit, which is how the screenshots above were made:

```sh
PERMAFROST_BUS=session PERMAFROST_SCREENSHOT=out.png PERMAFROST_SECTION=filters cargo run -p permafrost
```

`PERMAFROST_SECTION` takes `schedules`, `filters`, `history`, `settings`, `custom`, `list:<id>` or `preview:<id>`. Set `ADW_DEBUG_ACCENT_COLOR=blue` and `ADW_DEBUG_COLOR_SCHEME=prefer-dark` to control the look.

### D-Bus interface

`permafrostd` serves `io.github.atayoez.Permafrost1` at `/io/github/atayoez/Permafrost1` on the system bus. Structured values are JSON strings (see `crates/common/src/model.rs`).

| Method | Arguments |
|---|---|
| `GetStatus` | → status JSON |
| `SaveList` / `DeleteList` | block list JSON / id |
| `SaveSchedule` / `DeleteSchedule` | schedule JSON / id |
| `SetSettings` | settings JSON |
| `RestoreDefaults` | |
| `StartFreeze` | list ids, seconds, locked, work minutes, break minutes (0 for no breaks) |
| `AddTime` / `StopFreeze` | seconds / — |

Signals: `StatusChanged(status)` and `AppBlocked(app_id, until)`. Calls that would loosen a lock fail with `org.freedesktop.DBus.Error.AccessDenied`.

## Credits

- Community lists by [Steven Black](https://github.com/StevenBlack/hosts) (MIT), [HaGeZi](https://github.com/hagezi/dns-blocklists) (GPL-3.0) and [The Block List Project](https://github.com/blocklistproject/Lists) (Unlicense). Permafrost downloads them from their projects; they aren't bundled.
- Built with [GTK](https://gtk.org), [libadwaita](https://gnome.pages.gitlab.gnome.org/libadwaita/), [gtk-rs](https://gtk-rs.org), [zbus](https://github.com/dbus2/zbus) and [ashpd](https://github.com/bilelmoussaoui/ashpd).
- Inspired by [Cold Turkey](https://getcoldturkey.com), which doesn't run on Linux.

## License

Permafrost is free software under the [GNU General Public License v3.0 or later](LICENSE).
