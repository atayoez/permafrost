#!/bin/sh
# Builds Permafrost as you, then installs it system-wide with sudo.
#   ./install.sh              build and install
#   ./install.sh --uninstall  remove it again
set -eu

PREFIX=${PREFIX:-/usr}
APP_ID=io.github.atayoez.Permafrost
BUS_NAME=io.github.atayoez.Permafrost1

cd "$(dirname "$0")"

# Build in your own environment (where cargo is on PATH), then hand over to root.
if [ "$(id -u)" -ne 0 ]; then
    if [ "${1:-}" != "--uninstall" ]; then
        cargo build --release
    fi
    exec sudo PREFIX="$PREFIX" "$0" "$@"
fi

if [ "${1:-}" = "--uninstall" ]; then
    systemctl disable --now permafrostd.service 2>/dev/null || true
    rm -f "$PREFIX/bin/permafrost" "$PREFIX/libexec/permafrostd" \
        "$PREFIX/lib/systemd/system/permafrostd.service" \
        "$PREFIX/share/dbus-1/system.d/$BUS_NAME.conf" \
        "$PREFIX/share/applications/$APP_ID.desktop" \
        "$PREFIX/share/metainfo/$APP_ID.metainfo.xml" \
        "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg" \
        "$PREFIX/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"
    systemctl daemon-reload
    echo "Permafrost removed. Its state is still in /var/lib/permafrost."
    exit 0
fi

if [ ! -x target/release/permafrost ] || [ ! -x target/release/permafrostd ]; then
    echo "Nothing built yet. Run ./install.sh without sudo so it can build first." >&2
    exit 1
fi

install -Dm755 target/release/permafrost "$PREFIX/bin/permafrost"
install -Dm755 target/release/permafrostd "$PREFIX/libexec/permafrostd"
install -Dm644 data/permafrostd.service "$PREFIX/lib/systemd/system/permafrostd.service"
install -Dm644 "data/$BUS_NAME.conf" "$PREFIX/share/dbus-1/system.d/$BUS_NAME.conf"
install -Dm644 "data/$APP_ID.desktop" "$PREFIX/share/applications/$APP_ID.desktop"
install -Dm644 "data/$APP_ID.metainfo.xml" "$PREFIX/share/metainfo/$APP_ID.metainfo.xml"
install -Dm644 "data/icons/hicolor/scalable/apps/$APP_ID.svg" "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "data/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg" "$PREFIX/share/icons/hicolor/symbolic/apps/$APP_ID-symbolic.svg"

if [ "$PREFIX" != /usr ]; then
    sed -i "s|/usr/libexec/permafrostd|$PREFIX/libexec/permafrostd|" "$PREFIX/lib/systemd/system/permafrostd.service"
fi

gtk-update-icon-cache -qtf "$PREFIX/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$PREFIX/share/applications" 2>/dev/null || true
systemctl reload dbus.service 2>/dev/null || systemctl reload dbus-broker.service 2>/dev/null || true
systemctl daemon-reload
systemctl enable permafrostd.service
systemctl restart permafrostd.service
echo "Permafrost installed. If it was open, quit it (Ctrl+Q) and open it again."
