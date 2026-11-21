# Source layout
- src/main.rs      CLI (gui, start, stop, on, off, status, country, diag, widgets)
- src/backend.rs   hotspot engine (hostapd, dnsmasq, NAT, virtual AP)
- src/gui.rs       egui GUI: Hotspot, Clients, Country, Logs, Widgets, System tabs
- src/widgets.rs   desktop detection, per-distro packages, widget install/remove
- widgets/         Plasma plasmoid, GNOME extension, DMS plugins (embedded in the binary)
- data/            polkit policy, .desktop file, icon
- install.sh / uninstall.sh
