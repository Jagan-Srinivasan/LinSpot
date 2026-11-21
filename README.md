# LinSpot

One-click Wi-Fi hotspot for any Linux distro. Rust + egui GUI. Standalone: no `create_ap` / `linux-wifi-hotspot`.
Workflow is a port of `ori.py`: detect AP+client concurrency -> virtual AP `hs0` -> **1)** disconnect Wi-Fi
**2)** hostapd + dnsmasq + NAT on **3)** reconnect the same Wi-Fi (same channel). Stop restores everything.

See also docs/INSTALL.md, docs/WIDGETS.md, docs/LAYOUT.md.

## Install
    sudo ./install.sh              # deps for your distro + build + install (needs internet for crates)
    sudo ./install.sh --deps-only  # just system packages
    sudo ./install.sh --binary ./linspot   # use a prebuilt binary
    sudo ./install.sh --widgets auto|plasma|gnome|dms|all|none   # desktop widgets (default auto)
    sudo ./uninstall.sh [--purge] [--remove-deps]

Build only: `cargo build --release` -> `target/release/linspot`.

## Desktop widgets (inside the same app)
LinSpot ships its widgets for **KDE Plasma 6**, **GNOME Shell 45+** and **DankMaterialShell**. Open the app ->
**Widgets** tab: it detects your desktop and distro, installs the missing system packages (administrator prompt)
and copies the widget into your home folder, then shows the steps to switch it on.
CLI equivalent: `linspot widgets status | install <plasma|gnome|dms|auto|all> | remove <t> | steps <t>`,
`sudo linspot widgets deps <t>`. The widgets call `linspot on|off|status`, so set SSID + password once in the app.
See `widgets/README.md`.

## Use
`linspot` opens the GUI (Hotspot / Clients / Country code / Logs / System). Admin rights come from polkit (pkexec).
CLI: `sudo linspot start --ssid Net --password 12345678 [--band 5] [--channel 36] [--debug]`, `sudo linspot stop`,
`sudo linspot country IN --persist`, `linspot status`, `linspot diag`.

## Logs
Every command and its output is logged with `--debug` (GUI: "Detailed debug log"). Logs tab: level filters, search,
copy/save, live GUI session, backend `session.log`, `hostapd.log`, `dnsmasq.log` and a diagnostics dump.
Files live in `/run/linspot/`.
