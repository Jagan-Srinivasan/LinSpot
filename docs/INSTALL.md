# Installing LinSpot

LinSpot is a Rust + egui program (`src/`). `cargo build --release` produces ONE binary, `linspot`, which contains
the GUI, the hotspot engine and all desktop widgets (`widgets/` is compiled in with `include_str!`).

## Install
    sudo ./install.sh                   # distro packages + cargo build + install + widgets for your desktop
    sudo ./install.sh --widgets none    # app only
    sudo ./install.sh --widgets gnome   # force one widget (plasma | gnome | dms | all)
    sudo ./install.sh --binary ./linspot --no-deps   # prebuilt binary
    sudo ./uninstall.sh [--purge] [--remove-deps]    # removes app, polkit policy, widgets

What install.sh does: detects the package manager (pacman, apt, dnf, zypper, apk, xbps, emerge), installs
hostapd/dnsmasq/iw/nftables/polkit + GUI libraries, installs Rust if missing, builds, installs
`$PREFIX/bin/linspot`, the polkit policy, desktop entry, icon, a NetworkManager "unmanaged hs0" rule, then the widgets.
Run it with `sudo` from your normal user account (so widgets go to your home).

## Widgets from the GUI
Open LinSpot -> **Widgets** tab. It shows your desktop/distro, installs missing packages (admin prompt) and the
widget files, and lists the enable steps. Same from the CLI: `linspot widgets status|install|remove|steps|deps`.
