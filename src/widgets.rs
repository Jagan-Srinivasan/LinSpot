//! Desktop widgets shipped inside the binary: KDE Plasma 6 plasmoid, GNOME Shell extension and
//! DankMaterialShell plugins. Detects the desktop, installs the needed system packages for the
//! current distro (root, via pkexec) and copies the widget files into the user's home.
//!
//! Std-only on purpose: it prints tagged lines ("[STEP] ...") that the GUI log understands.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Plasma,
    Gnome,
    Dms,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::Plasma, Target::Gnome, Target::Dms];

    pub fn id(self) -> &'static str {
        match self {
            Target::Plasma => "plasma",
            Target::Gnome => "gnome",
            Target::Dms => "dms",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Target::Plasma => "KDE Plasma widget",
            Target::Gnome => "GNOME Shell extension",
            Target::Dms => "DankMaterialShell plugins",
        }
    }
    pub fn summary(self) -> &'static str {
        match self {
            Target::Plasma => "Panel icon with device badge and a popup with an on/off switch and the list of connected devices (Plasma 6).",
            Target::Gnome => "Quick Settings 'Hotspot' toggle with a menu of connected devices and a panel indicator while the hotspot is on (GNOME 45+).",
            Target::Dms => "Bar pill, Control Center tile and launcher entries (type 'hs') for DankMaterialShell.",
        }
    }
    pub fn parse(s: &str) -> Option<Target> {
        match s.to_ascii_lowercase().as_str() {
            "plasma" | "kde" => Some(Target::Plasma),
            "gnome" => Some(Target::Gnome),
            "dms" | "dank" | "dankmaterialshell" => Some(Target::Dms),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ embedded files
type Files = &'static [(&'static str, &'static str)];

const PLASMA_ID: &str = "org.linspot.hotspot";
const PLASMA_FILES: Files = &[
    ("metadata.json", include_str!("../widgets/plasma/org.linspot.hotspot/metadata.json")),
    ("contents/ui/main.qml", include_str!("../widgets/plasma/org.linspot.hotspot/contents/ui/main.qml")),
];

const GNOME_UUID: &str = "linspot@linspot.org";
const GNOME_FILES: Files = &[
    ("metadata.json", include_str!("../widgets/gnome/linspot@linspot.org/metadata.json")),
    ("extension.js", include_str!("../widgets/gnome/linspot@linspot.org/extension.js")),
];

const DMS_PLUGINS: [&str; 2] = ["linspotHotspot", "linspotLauncher"];
const DMS_FILES: Files = &[
    ("linspotHotspot/plugin.json", include_str!("../widgets/dms/linspotHotspot/plugin.json")),
    ("linspotHotspot/LinspotWidget.qml", include_str!("../widgets/dms/linspotHotspot/LinspotWidget.qml")),
    ("linspotHotspot/LinspotSettings.qml", include_str!("../widgets/dms/linspotHotspot/LinspotSettings.qml")),
    ("linspotHotspot/LsButton.qml", include_str!("../widgets/dms/linspotHotspot/LsButton.qml")),
    ("linspotLauncher/plugin.json", include_str!("../widgets/dms/linspotLauncher/plugin.json")),
    ("linspotLauncher/LinspotLauncher.qml", include_str!("../widgets/dms/linspotLauncher/LinspotLauncher.qml")),
];

/// Official DankMaterialShell installer (interactive, run by the user in a terminal).
pub const DMS_INSTALL_CMD: &str = "curl -fsSL https://install.danklinux.com | sh";

// ------------------------------------------------------------------ small helpers
fn out(tag: &str, msg: &str) {
    for l in msg.lines() {
        println!("[{}] {}", tag, l);
    }
}

fn find(tool: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in ["/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        dirs.push(PathBuf::from(d));
    }
    dirs.into_iter().map(|d| d.join(tool)).find(|p| p.is_file())
}

fn have(tool: &str) -> bool {
    find(tool).is_some()
}

fn capture(args: &[&str]) -> Option<String> {
    let exe = find(args[0])?;
    let o = Command::new(exe).args(&args[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

fn is_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(|l| l.to_string()))
        .and_then(|l| l.split_whitespace().nth(2).map(|v| v == "0"))
        .unwrap_or(false)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_dir())
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(".local/share")))
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(".config")))
}

/// Path of the installed `linspot` binary that the widgets must call. It has to be the path the
/// polkit policy was installed for (/etc/linspot/install.info), otherwise pkexec refuses it.
pub fn bin_path() -> String {
    if let Ok(s) = fs::read_to_string("/etc/linspot/install.info") {
        for l in s.lines() {
            if let Some(d) = l.strip_prefix("BINDIR=") {
                let p = format!("{}/linspot", d.trim());
                if Path::new(&p).is_file() {
                    return p;
                }
            }
        }
    }
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "linspot".into())
}

fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'").replace('"', "\\\"")
}

// ------------------------------------------------------------------ desktop detection
#[derive(Clone, Default, Debug)]
pub struct Detect {
    /// raw $XDG_CURRENT_DESKTOP
    pub xdg: String,
    pub session: String,
    pub plasma: bool,
    pub gnome: bool,
    pub dms: bool,
    pub plasma_ver: Option<u32>,
    pub gnome_ver: Option<u32>,
}

fn first_number(s: &str) -> Option<u32> {
    let mut it = s.split(|c: char| !c.is_ascii_digit()).filter(|t| !t.is_empty());
    it.next().and_then(|t| t.parse().ok())
}

pub fn detect() -> Detect {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let up = xdg.to_ascii_uppercase();
    let parts: Vec<&str> = up.split(':').collect();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let plasma = parts.contains(&"KDE");
    let gnome = parts.iter().any(|p| *p == "GNOME" || *p == "GNOME-CLASSIC");
    let cfg = config_home();
    let dms_present = have("dms")
        || cfg.as_ref().map(|c| c.join("DankMaterialShell").is_dir() || c.join("dms").is_dir()).unwrap_or(false)
        || Path::new("/usr/share/quickshell/dms").is_dir();
    // DMS is a shell that runs on top of compositors (niri, Hyprland, Sway ...), so it counts as
    // "detected" when it is installed and the session is not Plasma/GNOME.
    let dms = dms_present && !plasma && !gnome;
    Detect {
        xdg,
        session,
        plasma,
        gnome,
        dms,
        plasma_ver: capture(&["plasmashell", "--version"]).and_then(|s| first_number(&s)),
        gnome_ver: capture(&["gnome-shell", "--version"]).and_then(|s| first_number(&s)),
    }
}

// ------------------------------------------------------------------ install locations
fn plasma_dir() -> Option<PathBuf> {
    data_home().map(|d| d.join("plasma/plasmoids").join(PLASMA_ID))
}
fn gnome_dir() -> Option<PathBuf> {
    data_home().map(|d| d.join("gnome-shell/extensions").join(GNOME_UUID))
}
/// DMS reads plugins from ~/.config/DankMaterialShell/plugins (older) or ~/.config/dms/plugins (newer).
fn dms_dirs() -> Vec<PathBuf> {
    match config_home() {
        Some(c) => vec![c.join("DankMaterialShell/plugins"), c.join("dms/plugins")],
        None => vec![],
    }
}

pub fn is_installed(t: Target) -> bool {
    match t {
        Target::Plasma => plasma_dir().map(|d| d.join("metadata.json").is_file()).unwrap_or(false),
        Target::Gnome => gnome_dir().map(|d| d.join("extension.js").is_file()).unwrap_or(false),
        Target::Dms => dms_dirs().iter().any(|d| d.join("linspotHotspot/plugin.json").is_file()),
    }
}

// ------------------------------------------------------------------ prerequisites
fn qt6_qml_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    for base in ["/usr/lib", "/usr/lib64", "/usr/lib/x86_64-linux-gnu", "/usr/lib/aarch64-linux-gnu", "/usr/local/lib"] {
        v.push(PathBuf::from(base).join("qt6/qml"));
    }
    v
}

fn plasma5support_present() -> bool {
    qt6_qml_dirs().iter().any(|d| d.join("org/kde/plasma/plasma5support").is_dir())
}

/// Human-readable list of what is missing for `t` (empty = ready).
pub fn missing(t: Target) -> Vec<String> {
    let mut m = vec![];
    match t {
        Target::Plasma => {
            if !have("plasmashell") {
                m.push("plasmashell (KDE Plasma 6)".into());
            }
            if !plasma5support_present() {
                m.push("plasma5support QML module (runs the widget's commands)".into());
            }
        }
        Target::Gnome => {
            if !have("gnome-shell") {
                m.push("gnome-shell".into());
            }
            if !have("gnome-extensions") {
                m.push("gnome-extensions tool".into());
            }
            if !have("gsettings") {
                m.push("gsettings (glib2 tools)".into());
            }
        }
        Target::Dms => {
            if !have("dms") {
                m.push("DankMaterialShell (dms)".into());
            }
            if !have("qs") && !have("quickshell") {
                m.push("quickshell".into());
            }
        }
    }
    m
}

/// System packages to try for `t` on package manager `pm` (installed one by one; unavailable
/// names are skipped). DMS is not packaged uniformly, so it has none: use DMS_INSTALL_CMD.
pub fn dep_packages(t: Target, pm: &str) -> Vec<&'static str> {
    match (t, pm) {
        (Target::Plasma, "pacman") => vec!["plasma-workspace", "plasma5support", "kpackage"],
        (Target::Plasma, "apt") => vec!["plasma-workspace", "qml6-module-org-kde-plasma-plasma5support", "kpackagetool6"],
        (Target::Plasma, "dnf") => vec!["plasma-workspace", "plasma5support", "kf6-kpackage"],
        (Target::Plasma, "zypper") => vec!["plasma6-workspace", "plasma6-plasma5support", "kf6-kpackage"],
        (Target::Plasma, "apk") => vec!["plasma-workspace", "plasma5support", "kpackage"],
        (Target::Plasma, "xbps") => vec!["plasma-workspace", "plasma5support", "kpackage"],
        (Target::Gnome, "pacman") => vec!["gnome-shell", "glib2"],
        (Target::Gnome, "apt") => vec!["gnome-shell", "gnome-shell-extension-prefs", "libglib2.0-bin"],
        (Target::Gnome, "dnf") => vec!["gnome-shell", "gnome-extensions-app", "glib2"],
        (Target::Gnome, "zypper") => vec!["gnome-shell", "gnome-extensions", "glib2-tools"],
        (Target::Gnome, "apk") => vec!["gnome-shell", "glib"],
        (Target::Gnome, "xbps") => vec!["gnome-shell", "glib"],
        _ => vec![],
    }
}

pub fn pkg_manager() -> &'static str {
    for (bin, name) in [("pacman", "pacman"), ("apt-get", "apt"), ("dnf", "dnf"), ("zypper", "zypper"), ("apk", "apk"), ("xbps-install", "xbps"), ("emerge", "portage")] {
        if have(bin) {
            return name;
        }
    }
    "unknown"
}

/// Steps the user must do to switch the widget on after installation.
pub fn enable_steps(t: Target) -> Vec<String> {
    match t {
        Target::Plasma => vec![
            "Right-click the panel (or desktop) and choose 'Enter Edit Mode' -> 'Add Widgets...'.".into(),
            "Search for 'LinSpot Hotspot' and click it (or drag it onto the panel).".into(),
            "Click the panel icon: use the switch to turn the hotspot on/off and see connected devices.".into(),
            "Not listed? Restart the shell:  systemctl --user restart plasma-plasmashell   (or log out and in).".into(),
        ],
        Target::Gnome => vec![
            "Log out and log back in (Wayland). On X11 you can instead press Alt+F2, type  r  and press Enter.".into(),
            "Enable it:  gnome-extensions enable linspot@linspot.org   (or use the Extensions app).".into(),
            "Open Quick Settings (top-right corner): the 'Hotspot' tile appears. Click it to turn on/off; the arrow shows connected devices.".into(),
            "If it shows an error state, run  journalctl -f -o cat /usr/bin/gnome-shell  and look for 'linspot'.".into(),
        ],
        Target::Dms => vec![
            "Restart the shell:  dms restart".into(),
            "DMS Settings -> Plugins -> 'Scan for Plugins', then switch ON 'LinSpot Hotspot' and 'LinSpot Launcher'.".into(),
            "Bar: DMS Settings -> Dank Bar -> add the widget 'LinSpot Hotspot'.".into(),
            "Control Center: open it, press the pencil/edit button and add the 'Hotspot' tile.".into(),
            "Launcher: open it and type  hs".into(),
            "Not showing? Run  dms run  in a terminal and look for 'LinSpot' errors.".into(),
        ],
    }
}

// ------------------------------------------------------------------ status for the GUI
#[derive(Clone, Debug)]
pub struct TargetInfo {
    pub target: Target,
    /// matches the desktop this session is running
    pub detected: bool,
    pub installed: bool,
    /// things still missing (see `missing`)
    pub missing: Vec<String>,
    /// packages `deps` would try to install (empty for DMS / unknown package manager)
    pub packages: Vec<&'static str>,
    pub note: String,
}

pub fn scan() -> (Detect, Vec<TargetInfo>) {
    let d = detect();
    let pm = pkg_manager();
    let mut v = vec![];
    for t in Target::ALL {
        let detected = match t {
            Target::Plasma => d.plasma,
            Target::Gnome => d.gnome,
            Target::Dms => d.dms,
        };
        let mut note = String::new();
        match t {
            Target::Plasma => {
                if let Some(ver) = d.plasma_ver {
                    if ver < 6 {
                        note = format!("Plasma {} found - this widget needs Plasma 6.", ver);
                    }
                }
            }
            Target::Gnome => {
                if let Some(ver) = d.gnome_ver {
                    if ver < 45 {
                        note = format!("GNOME Shell {} found - this extension needs GNOME 45 or newer.", ver);
                    }
                }
            }
            Target::Dms => {}
        }
        v.push(TargetInfo { target: t, detected, installed: is_installed(t), missing: missing(t), packages: dep_packages(t, pm), note });
    }
    // detected desktop first
    v.sort_by_key(|i| !i.detected);
    (d, v)
}

// ------------------------------------------------------------------ install / remove (user level)
fn write_files(root: &Path, files: Files, bin: Option<&str>) -> Result<(), String> {
    for (rel, content) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        let text = match bin {
            Some(b) => content.replace("@LINSPOT_BIN@", &js_escape(b)),
            None => content.to_string(),
        };
        fs::write(&p, text).map_err(|e| format!("{}: {}", p.display(), e))?;
    }
    Ok(())
}

fn gsettings_enabled() -> Option<Vec<String>> {
    let s = capture(&["gsettings", "get", "org.gnome.shell", "enabled-extensions"])?;
    Some(s.split('\'').enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, v)| v.to_string()).collect())
}

fn gnome_enable() -> bool {
    // Preferred: ask the running shell. It refuses extensions it has not discovered yet
    // (new installs are only picked up at the next login on Wayland) ...
    if let Some(exe) = find("gnome-extensions") {
        if Command::new(exe).args(["enable", GNOME_UUID]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    // ... so also record it in the enabled list: GNOME enables it as soon as it loads it.
    let Some(mut list) = gsettings_enabled() else { return false };
    if !list.iter().any(|u| u == GNOME_UUID) {
        list.push(GNOME_UUID.to_string());
    }
    let val = format!("[{}]", list.iter().map(|u| format!("'{}'", u)).collect::<Vec<_>>().join(", "));
    Command::new("gsettings").args(["set", "org.gnome.shell", "enabled-extensions", &val]).status().map(|s| s.success()).unwrap_or(false)
}

fn gnome_disable() {
    if let Some(exe) = find("gnome-extensions") {
        let _ = Command::new(exe).args(["disable", GNOME_UUID]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    if let Some(list) = gsettings_enabled() {
        if list.iter().any(|u| u == GNOME_UUID) {
            let rest: Vec<String> = list.into_iter().filter(|u| u != GNOME_UUID).collect();
            let val = if rest.is_empty() { "@as []".to_string() } else { format!("[{}]", rest.iter().map(|u| format!("'{}'", u)).collect::<Vec<_>>().join(", ")) };
            let _ = Command::new("gsettings").args(["set", "org.gnome.shell", "enabled-extensions", &val]).status();
        }
    }
}

pub fn install(t: Target) -> Result<(), String> {
    if is_root() {
        return Err("widgets install into your home directory: run this as your normal user, not root".into());
    }
    let bin = bin_path();
    out("STEP", &format!("Installing {} (linspot binary: {})", t.title(), bin));
    match t {
        Target::Plasma => {
            let d = plasma_dir().ok_or("cannot determine the data directory ($HOME not set)")?;
            let _ = fs::remove_dir_all(&d);
            write_files(&d, PLASMA_FILES, Some(&bin))?;
            out("INFO", &format!("wrote {}", d.display()));
            // refresh KDE's plugin cache (best effort)
            for b in ["kbuildsycoca6", "kbuildsycoca5"] {
                if let Some(exe) = find(b) {
                    let _ = Command::new(exe).arg("--noincremental").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
                    break;
                }
            }
        }
        Target::Gnome => {
            let d = gnome_dir().ok_or("cannot determine the data directory ($HOME not set)")?;
            let _ = fs::remove_dir_all(&d);
            write_files(&d, GNOME_FILES, Some(&bin))?;
            out("INFO", &format!("wrote {}", d.display()));
            if gnome_enable() {
                out("INFO", "extension marked as enabled");
            } else {
                out("WARN", "could not mark the extension as enabled - enable it manually (see the steps below)");
            }
        }
        Target::Dms => {
            let dirs = dms_dirs();
            if dirs.is_empty() {
                return Err("cannot determine the config directory ($HOME not set)".into());
            }
            for d in dirs {
                for p in DMS_PLUGINS {
                    let _ = fs::remove_dir_all(d.join(p));
                }
                write_files(&d, DMS_FILES, None)?;
                out("INFO", &format!("wrote {}/{{{}}}", d.display(), DMS_PLUGINS.join(",")));
            }
        }
    }
    let cfg = config_home().map(|c| c.join("linspot/config.json"));
    if !cfg.map(|c| c.is_file()).unwrap_or(false) {
        out("WARN", "no saved hotspot settings yet: open LinSpot, enter a network name + password (8+ chars) and press Start once");
    }
    out("OK", &format!("{} installed", t.title()));
    for (i, s) in enable_steps(t).iter().enumerate() {
        out("INFO", &format!("{}. {}", i + 1, s));
    }
    Ok(())
}

pub fn remove(t: Target) -> Result<(), String> {
    if is_root() {
        return Err("run this as your normal user, not root".into());
    }
    out("STEP", &format!("Removing {}", t.title()));
    match t {
        Target::Plasma => {
            if let Some(d) = plasma_dir() {
                let _ = fs::remove_dir_all(d);
            }
        }
        Target::Gnome => {
            gnome_disable();
            if let Some(d) = gnome_dir() {
                let _ = fs::remove_dir_all(d);
            }
        }
        Target::Dms => {
            for d in dms_dirs() {
                for p in DMS_PLUGINS {
                    let _ = fs::remove_dir_all(d.join(p));
                }
            }
        }
    }
    out("OK", &format!("{} removed", t.title()));
    match t {
        Target::Plasma => out("INFO", "Remove the widget from your panel if it is still shown (right-click -> Remove)."),
        Target::Gnome => out("INFO", "Log out and in (or Alt+F2, r on X11) to unload it."),
        Target::Dms => out("INFO", "Run: dms restart"),
    }
    Ok(())
}

// ------------------------------------------------------------------ system packages (root)
fn pm_cmd(pm: &str, pkg: &str) -> Option<Vec<String>> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).chain(std::iter::once(pkg.to_string())).collect();
    Some(match pm {
        "pacman" => v(&["pacman", "-S", "--needed", "--noconfirm"]),
        "apt" => v(&["apt-get", "install", "-y", "-qq"]),
        "dnf" => v(&["dnf", "install", "-y", "-q"]),
        "zypper" => v(&["zypper", "--non-interactive", "install", "-y"]),
        "apk" => v(&["apk", "add", "--no-progress"]),
        "xbps" => v(&["xbps-install", "-Sy"]),
        "portage" => v(&["emerge", "--noreplace", "--quiet"]),
        _ => return None,
    })
}

/// Install the system packages `t` needs. Must run as root (the GUI starts it through pkexec).
pub fn deps(t: Target) -> Result<(), String> {
    if !is_root() {
        return Err("installing packages needs administrator rights (the GUI asks through pkexec; from a terminal use sudo)".into());
    }
    if t == Target::Dms {
        out("INFO", "DankMaterialShell is not installed from distro packages here. Run this as your normal user in a terminal:");
        out("INFO", DMS_INSTALL_CMD);
        out("INFO", "or see https://danklinux.com for your distribution.");
        return Ok(());
    }
    let pm = pkg_manager();
    let pkgs = dep_packages(t, pm);
    if pkgs.is_empty() {
        return Err(format!("no package list for package manager '{}': install manually: {}", pm, missing(t).join(", ")));
    }
    out("STEP", &format!("Installing packages for {} with {}", t.title(), pm));
    if pm == "apt" {
        if let Some(e) = find("apt-get") {
            let _ = Command::new(e).args(["update", "-qq"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
    let mut failed = vec![];
    for p in &pkgs {
        let Some(c) = pm_cmd(pm, p) else { break };
        let Some(exe) = find(&c[0]) else { continue };
        let mut cmd = Command::new(exe);
        cmd.args(&c[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if pm == "apt" {
            cmd.env("DEBIAN_FRONTEND", "noninteractive");
        }
        if cmd.status().map(|s| s.success()).unwrap_or(false) {
            out("INFO", &format!("+ {}", p));
        } else {
            out("INFO", &format!("- {} (unavailable on this distro, skipped)", p));
            failed.push(*p);
        }
    }
    let still = missing(t);
    if still.is_empty() {
        out("OK", "all prerequisites present");
    } else {
        out("WARN", &format!("still missing: {}", still.join(", ")));
        if !failed.is_empty() {
            out("WARN", &format!("could not install: {}", failed.join(", ")));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ CLI entry
pub const HELP: &str = "linspot widgets - desktop widgets for KDE Plasma, GNOME and DankMaterialShell

  linspot widgets status                 detected desktop and per-widget state
  linspot widgets install <t|auto|all>   install widget(s) into your home (no root)
  linspot widgets remove  <t|all>        remove widget(s)
  linspot widgets steps   <t>            how to enable the widget
  sudo linspot widgets deps <t>          install the system packages the widget needs

  <t> = plasma | gnome | dms.  'auto' = the desktop this session runs (falls back to
  every widget whose shell is installed).";

fn targets_arg(a: Option<&str>, auto: bool) -> Result<Vec<Target>, String> {
    match a {
        Some("all") => Ok(Target::ALL.to_vec()),
        Some("auto") if auto => {
            let (_d, infos) = scan();
            let hit: Vec<Target> = infos.iter().filter(|i| i.detected).map(|i| i.target).collect();
            if !hit.is_empty() {
                return Ok(hit);
            }
            // not inside a known desktop session (e.g. run from a root shell / ssh): use what is installed
            let any: Vec<Target> = infos.iter().filter(|i| i.missing.is_empty()).map(|i| i.target).collect();
            Ok(any)
        }
        Some(s) => Target::parse(s).map(|t| vec![t]).ok_or_else(|| format!("unknown widget target '{}' (use plasma, gnome, dms, auto or all)", s)),
        None => Err("missing target (plasma | gnome | dms | auto | all)".into()),
    }
}

pub fn cli(args: &[String]) -> i32 {
    let a1 = args.get(1).map(|s| s.as_str());
    let result: Result<(), String> = match args.first().map(|s| s.as_str()) {
        Some("status") => {
            let (d, infos) = scan();
            println!("desktop: {}  session: {}  package manager: {}", if d.xdg.is_empty() { "(unknown)" } else { &d.xdg }, if d.session.is_empty() { "(unknown)" } else { &d.session }, pkg_manager());
            for i in infos {
                println!(
                    "{:<7} detected={} installed={} missing=[{}]{}",
                    i.target.id(),
                    i.detected,
                    i.installed,
                    i.missing.join(", "),
                    if i.note.is_empty() { String::new() } else { format!("  note: {}", i.note) }
                );
            }
            Ok(())
        }
        Some("install") => targets_arg(a1, true).and_then(|ts| {
            if ts.is_empty() {
                out("WARN", "no supported desktop detected - nothing installed. Use: linspot widgets install plasma|gnome|dms");
                return Ok(());
            }
            let mut err = vec![];
            for t in ts {
                if let Err(e) = install(t) {
                    err.push(format!("{}: {}", t.id(), e));
                }
            }
            if err.is_empty() { Ok(()) } else { Err(err.join("; ")) }
        }),
        Some("remove") => targets_arg(a1, false).and_then(|ts| {
            for t in ts {
                remove(t)?;
            }
            Ok(())
        }),
        Some("steps") => targets_arg(a1, false).map(|ts| {
            for t in ts {
                println!("{}:", t.title());
                for (i, s) in enable_steps(t).iter().enumerate() {
                    println!("  {}. {}", i + 1, s);
                }
            }
        }),
        Some("deps") => targets_arg(a1, false).and_then(|ts| {
            for t in ts {
                deps(t)?;
            }
            Ok(())
        }),
        _ => {
            println!("{}", HELP);
            return 0;
        }
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            out("ERROR", &e);
            1
        }
    }
}
