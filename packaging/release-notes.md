## LinSpot ${TAG}

One-click Wi-Fi hotspot for Linux — now with QR connect codes and device management.

### New in this release
- **QR sharing**: a *Share* tab in the app renders a Wi-Fi connect QR (Android/iOS/Windows
  scan it and join without typing the password); "Save QR as PNG" and "Copy Wi-Fi connect
  string" included. The desktop widgets (Plasma, GNOME, DankMaterialShell) can show the QR
  too, and `linspot qr --png file.png` writes it from any terminal or script.
- **Disconnect devices**: kick a connected device from the Clients tab, or with
  `linspot kick <MAC>`.
- **GUI refresh**: hero status bar with live uptime, icon sidebar, quick actions, softer
  cards.
- Pure-Rust QR encoder built in — no new runtime dependencies.

### Install

| Distro | File | Command |
|---|---|---|
| Debian / Ubuntu / Mint | `linspot_*_amd64.deb` | `sudo apt install ./linspot_*_amd64.deb` |
| Fedora / RHEL | `linspot-*.x86_64.rpm` | `sudo dnf install ./linspot-*.x86_64.rpm` |
| Arch / CachyOS / Manjaro | `linspot-*.pkg.tar.zst` | `sudo pacman -U ./linspot-*.pkg.tar.zst` |
| Any Linux (x86_64) | `linspot-*-x86_64-linux.tar.gz` | extract and run `./linspot` |

Packages pull all runtime dependencies automatically — dependency names are verified per
distro inside CI containers on every release (on Debian/Ubuntu the polkit stack is
`polkitd` + `pkexec`; on Fedora/openSUSE/Arch it is `polkit`). The .deb targets
Debian 12 / Ubuntu 22.04 and newer, the .rpm targets Fedora / RHEL. On other RPM distros
(openSUSE uses different library names, e.g. `Mesa-libGL1`) or when building from source
use `sudo ./install.sh`.

After installing, run `linspot`, set a network name and password and press Start.
See the README for widgets, CLI usage and the country-code tab.
