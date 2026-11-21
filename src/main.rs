//! LinSpot - standalone Wi-Fi hotspot (hostapd + dnsmasq + NAT), egui front-end.
//!
//!   linspot                 open the GUI
//!   sudo linspot start --ssid MyNet --password 12345678 [--iface wlan0] [--channel 6]
//!                      [--band auto|2.4|5] [--country IN] [--debug]
//!   sudo linspot stop
//!   sudo linspot country IN [--persist]
//!   linspot on | off        start/stop with the saved GUI settings (used by the desktop widgets)
//!   linspot widgets ...     install KDE Plasma / GNOME / DankMaterialShell widgets
//!   linspot status | diag | help
mod backend;
mod countries;
mod gui;
mod widgets;

use backend::Config;
use std::io::Read;

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        None | Some("gui") => gui::run(),
        Some("start") => cmd_start(&args[1..]),
        Some("stop") => {
            backend::set_log_file(true);
            report(backend::stop(false))
        }
        Some("status") => {
            println!("{}", serde_json::to_string(&backend::status()).unwrap_or_default());
            0
        }
        Some("country") => cmd_country(&args[1..]),
        Some("on") => cmd_on(),
        Some("off") => cmd_off(),
        Some("widgets") => widgets::cli(&args[1..]),
        Some("diag") => {
            for l in backend::diagnostics() {
                println!("{}", l);
            }
            0
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("linspot {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("-h") | Some("--help") | Some("help") => {
            print!("{}", HELP);
            0
        }
        Some(x) => {
            eprintln!("unknown command '{}'\n\n{}", x, HELP);
            2
        }
    }
}

const HELP: &str = "LinSpot - Wi-Fi hotspot for any Linux distro

  linspot                         open the GUI
  sudo linspot start --ssid NAME --password PASS [--iface IF] [--channel N]
                     [--band auto|2.4|5] [--country XX] [--debug]
  sudo linspot stop
  sudo linspot country XX [--persist]   set regulatory country (e.g. IN, US)
  linspot country                       show current regulatory country
  linspot on | off                      start / stop using the settings saved by the GUI
  linspot widgets ...                   desktop widgets (Plasma, GNOME, DankMaterialShell)
  linspot status                        JSON status
  linspot diag                          print diagnostics
";

fn report(r: Result<(), String>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            backend::error(&e);
            1
        }
    }
}

fn take(a: &[String], i: &mut usize) -> Option<String> {
    *i += 1;
    a.get(*i).cloned()
}

fn cmd_start(a: &[String]) -> i32 {
    let mut cfg = Config::default();
    let mut from_stdin = false;
    let mut i = 0;
    while i < a.len() {
        let key = a[i].clone();
        match key.as_str() {
            "--ssid" => cfg.ssid = take(a, &mut i).unwrap_or_default(),
            "--password" => cfg.password = take(a, &mut i).unwrap_or_default(),
            "--iface" => cfg.iface = take(a, &mut i),
            "--channel" => cfg.channel = take(a, &mut i).and_then(|v| v.parse().ok()),
            "--country" => cfg.country = take(a, &mut i),
            "--band" => cfg.band = take(a, &mut i),
            "--stdin" => from_stdin = true,
            "--debug" => backend::set_debug(true),
            other => {
                eprintln!("unknown option {}", other);
                return 2;
            }
        }
        i += 1;
    }
    if from_stdin {
        let mut s = String::new();
        let _ = std::io::stdin().read_to_string(&mut s);
        match serde_json::from_str::<Config>(&s) {
            Ok(c) => cfg = normalize(c),
            Err(e) => {
                backend::error(&format!("bad config on stdin: {}", e));
                return 1;
            }
        }
    }
    if cfg.ssid.is_empty() || cfg.password.is_empty() {
        eprintln!("--ssid and --password are required");
        return 2;
    }
    backend::set_log_file(true);
    report(backend::start(&cfg))
}

fn cmd_country(a: &[String]) -> i32 {
    if a.is_empty() {
        println!("{}", backend::reg_country().unwrap_or_else(|| "00 (world / unset)".into()));
        return 0;
    }
    let persist = a.iter().any(|x| x == "--persist");
    backend::set_log_file(true);
    report(backend::set_country(&a[0], persist))
}

/// The GUI's settings file uses 0 / "" for "automatic"; the backend wants None.
fn normalize(mut c: Config) -> Config {
    if c.channel == Some(0) {
        c.channel = None;
    }
    if c.iface.as_deref() == Some("") {
        c.iface = None;
    }
    if c.band.as_deref() == Some("") {
        c.band = None;
    }
    c
}

/// Run `exe args...` as root: directly when already root, otherwise through pkexec.
fn elevated(args: &[&str], stdin_data: Option<&str>) -> i32 {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            backend::error(&format!("cannot locate own binary: {}", e));
            return 1;
        }
    };
    let mut cmd = if backend::is_root() {
        Command::new(&exe)
    } else if let Some(pk) = backend::find("pkexec") {
        let mut c = Command::new(pk);
        c.arg(&exe);
        c
    } else {
        backend::error("pkexec (polkit) not found - run with sudo instead");
        return 127;
    };
    cmd.args(args).stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            backend::error(&format!("cannot launch: {}", e));
            return 1;
        }
    };
    if let Some(d) = stdin_data {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(d.as_bytes());
        }
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(-1)
}

/// `linspot on`: start the hotspot with the settings the GUI saved (~/.config/linspot/config.json).
fn cmd_on() -> i32 {
    let raw = match std::fs::read_to_string(gui::cfg_path()) {
        Ok(s) => s,
        Err(_) => {
            backend::error("no saved settings: open the LinSpot app, enter a network name and password, press Start once");
            return 1;
        }
    };
    let cfg: Config = match serde_json::from_str::<Config>(&raw) {
        Ok(c) => normalize(c),
        Err(e) => {
            backend::error(&format!("bad settings file {}: {}", gui::cfg_path().display(), e));
            return 1;
        }
    };
    if cfg.ssid.is_empty() || !(8..=63).contains(&cfg.password.len()) {
        backend::error("saved settings are incomplete: need a network name and a password of 8-63 characters");
        return 1;
    }
    let json = serde_json::to_string(&cfg).unwrap_or_default();
    elevated(&["start", "--stdin"], Some(&json))
}

fn cmd_off() -> i32 {
    elevated(&["stop"], None)
}
