# LinSpot desktop widgets

All widget sources live here and are compiled into the `linspot` binary (`include_str!`), so the single
GUI app can install them: **Widgets** tab, or `linspot widgets install auto`.

| Desktop | Source | Installed to | Gives you |
|---|---|---|---|
| KDE Plasma 6 | `plasma/org.linspot.hotspot` | `~/.local/share/plasma/plasmoids/` | panel icon + device badge, popup with on/off switch and device list |
| GNOME Shell 45-50 | `gnome/linspot@linspot.org` | `~/.local/share/gnome-shell/extensions/` | Quick Settings "Hotspot" toggle + menu, panel indicator while on |
| DankMaterialShell | `dms/` | `~/.config/DankMaterialShell/plugins/` and `~/.config/dms/plugins/` | bar pill, Control Center tile, launcher (`hs`) |

Plasma/GNOME widgets call `linspot status`, `linspot on`, `linspot off` and `linspot gui`; the binary path is
written into the widget at install time (it must be the path the polkit policy was installed for).
`on`/`off` use the settings saved by the app (`~/.config/linspot/config.json`) and ask for admin rights via polkit.

System packages are installed per distro (pacman, apt, dnf, zypper, apk, xbps) and only for the desktop you
are running. DankMaterialShell itself is installed with its official installer (`curl -fsSL https://install.danklinux.com | sh`).
