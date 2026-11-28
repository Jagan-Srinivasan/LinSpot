# Widgets

| Desktop | Needs | Enable |
|---|---|---|
| KDE Plasma 6 | plasma-workspace, plasma5support | Edit Mode -> Add Widgets -> "LinSpot Hotspot" |
| GNOME 45-50 | gnome-shell, gnome-extensions, gsettings | re-login (Wayland), `gnome-extensions enable linspot@linspot.org`, Quick Settings tile |
| DankMaterialShell | dms + quickshell (`curl -fsSL https://install.danklinux.com \| sh`) | `dms restart`, Settings -> Plugins -> enable, add to bar / Control Center |

Widgets call `linspot status|on|off|gui|qr`. `on` uses the settings saved by the app (~/.config/linspot/config.json),
so save SSID + password once in the app. Admin rights come from polkit.
Starting still asks for authentication; **turning off and disconnecting devices is passwordless**
via a sudoers rule (`/etc/sudoers.d/50-linspot`) that the installer and packages set up.
Every widget can also show the **Wi-Fi connect QR** (Plasma: 'Show QR' button in the popup; GNOME:
'Show Wi-Fi QR' in the toggle menu; DMS: button in the popout and 'Show Wi-Fi QR' entry in the launcher
under `hs`) — it is generated with `linspot qr --png` and opens as an image, no admin rights needed.
Sources: `widgets/plasma`, `widgets/gnome`, `widgets/dms`; install logic: `src/widgets.rs`.
Known limits: Plasma 5 unsupported; untested on live Plasma/GNOME/DMS sessions - report errors from `journalctl`/`dms run`.
