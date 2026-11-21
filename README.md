<div align="center">

# 📡 LinSpot

**One-click Wi-Fi hotspot for any Linux distro — with a modern GUI and built-in desktop widgets.**

![Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust&logoColor=white)
![GUI](https://img.shields.io/badge/GUI-egui-38bdf8?logoColor=white)
![Distros](https://img.shields.io/badge/Distros-Arch%20%7C%20Manjaro%20%7C%20EndeavourOS%20%7C%20Debian%20%7C%20Ubuntu%20%7C%20Linux%20Mint-green?logoColor=white)
![Widgets](https://img.shields.io/badge/Widgets-KDE%20Plasma%20%7C%20GNOME%20%7C%20DankMaterialShell-a78bfa?logoColor=white)

[Features](#-why-linspot) · [Screenshots](#-screenshots) · [Widgets](#-desktop-widgets) · [Install](#-install) · [Usage](#-usage) · [Docs](#-documentation)

</div>

---

## ✨ Why LinSpot

Standalone: **no `create_ap`, no `linux-wifi-hotspot`** — just one Rust binary that drives `hostapd`, `dnsmasq` and NAT itself.

| | Advantage | What it means for you |
|---|---|---|
| 🦀 | **Single Rust binary** | Fast, small, no Python/Electron. GUI, engine and CLI in one `linspot` executable |
| 🔁 | **Share Wi-Fi while connected to Wi-Fi** | Detects AP + client concurrency and creates a virtual AP `hs0`, so your laptop keeps its Wi-Fi *and* shares it (same channel) |
| 🔒 | **GUI never runs as root** | Admin work goes through **polkit (`pkexec`)** — you only see a normal password prompt |
| ♻️ | **Stop restores everything** | Wi-Fi connection, firewall/NAT rules, NetworkManager state are put back |
| 🌍 | **Works on any distro** | Installer detects pacman, apt, dnf, zypper, apk, xbps, emerge and installs the right packages |
| 🧩 | **Widgets built into the app** | KDE Plasma, GNOME and DankMaterialShell widgets are installed and enabled from the **Widgets tab** — no terminal |
| 📶 | **Band / channel / country control** | 2.4 or 5 GHz, auto or fixed channel, regulatory country picker (fixes blocked 5 GHz) |
| 👥 | **Live client list** | See every connected device (name, IP) |
| 🪵 | **Deep logging** | Every command and its output, filterable, searchable, exportable |
| 📦 | **deb / rpm packaging** | Ready-made packaging scripts; the package manager installs all dependencies |

**How it works** (a port of `ori.py`): detect AP+client concurrency → create virtual AP `hs0` → **1)** disconnect Wi-Fi → **2)** hostapd + dnsmasq + NAT on → **3)** reconnect the same Wi-Fi (same channel). Stop restores everything.

---

## 🖼️ Screenshots

### The app
<img width="1919" height="1078" alt="screenshot_2026-11-21_10-25-26" src="https://github.com/user-attachments/assets/74db2ab0-49ab-4ff7-a1e5-1d8125524f21" />


<table>
  <tr>
    <td width="33%"><img src="https://github.com/user-attachments/assets/96c3ddd1-ed4d-425a-87b8-e8300687c0ff" alt="LinSpot screenshot 1"/><br/><sub><b>Hotspot</b> — SSID, password, band, channel, one-click Start</sub></td>
    <td width="33%"><img src="https://github.com/user-attachments/assets/23bf59f8-23f2-4db4-b766-5674111cabd0" alt="LinSpot screenshot 2"/><br/><sub><b>Running hotspot</b> — live status and connected devices</sub></td>
    <td width="33%"><img src="https://github.com/user-attachments/assets/ccb9119e-1db3-449e-82bf-2a463f1a1f2e" alt="LinSpot screenshot 3"/><br/><sub><b>Country code</b> — regulatory domain picker</sub></td>
  </tr>
  <tr>
    <td width="33%"><img src="https://github.com/user-attachments/assets/19cd1b1f-9e7b-443d-a180-d4918af407b2" alt="LinSpot screenshot 4"/><br/><sub><b>Logs</b> — filters, search, backend logs</sub></td>
    <td width="33%"><img src="https://github.com/user-attachments/assets/c4060336-f014-444c-92ec-99d5615ba84f" alt="LinSpot screenshot 5"/><br/><sub><b>Widgets tab</b> — detect desktop, install, enable</sub></td>
    <td width="33%"><img src="https://github.com/user-attachments/assets/0d57ce46-7ae4-49b1-9fc9-c02c9e208e49" alt="LinSpot screenshot 6"/><br/><sub><b>System</b> — dependencies and Wi-Fi adapters</sub></td>
  </tr>
</table>

### The widgets

<table>
  <tr>
    <td width="30%"><img src="https://github.com/user-attachments/assets/84a244a8-70ad-4ad8-9393-61b0df2ef248" alt="Widget screenshot 1"/></td>
    <td width="30%"><img src="https://github.com/user-attachments/assets/439158cc-0e0a-4b00-9a28-36037dc8abb1" alt="Widget screenshot 2"/></td>
    <td width="40%"><img src="https://github.com/user-attachments/assets/d05e6d3f-3127-4d7e-a227-8296350436b1" alt="Widget screenshot 3"/></td>
  </tr>
</table>

---

## 🧩 Desktop widgets

> **Control your hotspot without opening the app.** LinSpot ships its own widgets for the three desktops below. They are **built into the binary** and installed from the app's **Widgets** tab.

| Desktop | Widget | What you get | Enable |
|---|---|---|---|
| 🟦 **KDE Plasma 6** | Plasma widget (plasmoid) | Panel icon with a **device-count badge**; popup with an **on/off switch** and the list of connected devices | One click: added to your panel automatically |
| 🟧 **GNOME Shell 45–50** | Shell extension | **Quick Settings "Hotspot" tile** with a device menu, plus a panel indicator while the hotspot is on | One click: extension enabled (loads at next login on Wayland) |
| 🟪 **DankMaterialShell** | Plugins | **Bar pill**, **Control Center tile** and **launcher entries** (type `hs`) | Install from the tab, then switch on in DMS Settings → Plugins |

### How the Widgets tab works

1. **Detects** your desktop, session type, distro and package manager.
2. **Installs missing system packages** for your distro (admin password via polkit — only the packages the widget needs, only for the desktop you are running).
3. **Installs the widget** into your home folder (or uses the copy shipped by the deb/rpm package).
4. **Enables it** where that can be automated, and shows **step-by-step instructions** ("How to enable it") for the rest.
5. **Remove** any widget from the same card.

All widgets call the same small CLI — `linspot status`, `linspot on`, `linspot off` — so **set your SSID and password once in the app**, then use any widget.

```bash
# CLI equivalent of the Widgets tab
linspot widgets status                         # detected desktop + widget state
linspot widgets install auto                   # widget for the desktop you are running
linspot widgets install plasma|gnome|dms|all
linspot widgets enable auto                    # Plasma: add to panel, GNOME: enable
linspot widgets steps gnome                    # how to switch it on
linspot widgets remove all
sudo linspot widgets deps plasma               # system packages for a widget
```

More in [`widgets/README.md`](widgets/README.md) and [`docs/WIDGETS.md`](docs/WIDGETS.md).

---

## 📥 Install

### Option A — deb / rpm package (no terminal work for users)

```bash
sudo apt install ./linspot_*_amd64.deb        # Debian / Ubuntu / Mint
sudo dnf install ./linspot-*.x86_64.rpm       # Fedora / RHEL   (zypper install on openSUSE)
```

The package manager installs **all dependencies** automatically; the Plasma widget and GNOME extension are shipped system-wide, so the **Widgets tab → Enable widget** is all that is left.
Build the packages with `./packaging/build-packages.sh` — see [`docs/PACKAGING.md`](docs/PACKAGING.md).

### Option B — from source (any distro)

```bash
sudo ./install.sh                  # distro dependencies + build + install + widgets for your desktop
sudo ./install.sh --deps-only      # only system packages
sudo ./install.sh --binary ./linspot            # use a prebuilt binary
sudo ./install.sh --widgets auto|plasma|gnome|dms|all|none
sudo ./uninstall.sh [--purge] [--remove-deps]
```

Build only: `cargo build --release` → `target/release/linspot`.
Run `install.sh` with `sudo` from your normal user so widgets go to *your* home.

---

## 🚀 Usage

**GUI:** run `linspot` (or open **LinSpot** from the app menu).

| Tab | Purpose |
|---|---|
| **Hotspot** | Network name, password, band, channel, interface; Start / Stop |
| **Clients** | Connected devices |
| **Country code** | Wi-Fi regulatory domain (needed for 5 GHz in many countries) |
| **Widgets** | Detect desktop, install / enable / remove widgets, instructions |
| **Logs** | Live session, `session.log`, `hostapd.log`, `dnsmasq.log`, diagnostics |
| **System** | Distro, package manager, dependency check, Wi-Fi adapters |

**CLI:**

```bash
sudo linspot start --ssid Net --password 12345678 [--band 5] [--channel 36] [--debug]
sudo linspot stop
sudo linspot country IN --persist
linspot on | off          # use the settings saved by the GUI (this is what widgets call)
linspot status            # JSON status
linspot diag              # diagnostics
```

---

## 🪵 Logs

Every command and its output is logged with `--debug` (GUI: "Detailed debug log"). Logs tab: level filters, search, copy/save, live GUI session, backend `session.log`, `hostapd.log`, `dnsmasq.log` and a diagnostics dump. Files live in `/run/linspot/`.

---

## 📚 Documentation

| File | Content |
|---|---|
| [`docs/INSTALL.md`](docs/INSTALL.md) | Installing and what `install.sh` does |
| [`docs/WIDGETS.md`](docs/WIDGETS.md) | Widgets per desktop, requirements, enabling |
| [`docs/PACKAGING.md`](docs/PACKAGING.md) | Building `.deb` / `.rpm` |
| [`docs/LAYOUT.md`](docs/LAYOUT.md) | Source layout |
| [`widgets/README.md`](widgets/README.md) | Widget sources and how they talk to `linspot` |

## ⚠️ Notes

- Plasma widget needs **Plasma 6**; GNOME extension needs **GNOME 45+**.
- DankMaterialShell itself is installed with its official installer: `curl -fsSL https://install.danklinux.com | sh`.

## 󰿃 License

LinSpot is licensed under the GNU General Public License v3.0.

Copyright (C) 2026 Jagan S.

See [LICENSE](LICENSE) for the full license text.
