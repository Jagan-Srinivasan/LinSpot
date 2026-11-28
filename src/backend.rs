//! Hotspot engine - a 1:1 port of the ori.py workflow:
//!   detect concurrency -> virtual AP (hs0) -> 1) disconnect Wi-Fi 2) hostapd+dnsmasq+NAT ON
//!   3) reconnect same Wi-Fi.  Stop tears everything down and restores the previous state.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, sleep};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const STATE_DIR: &str = "/run/linspot";
pub const STATE_FILE: &str = "/run/linspot/state.json";
pub const SESSION_LOG: &str = "/run/linspot/session.log";
pub const AP_IF: &str = "hs0";
pub const SUBNET: &str = "192.168.12";
pub const GW: &str = "192.168.12.1";
pub const COUNTRY_FILE: &str = "/etc/linspot/country";

pub type Res<T> = Result<T, String>;

// ------------------------------------------------------------------ logging
static DEBUG: AtomicBool = AtomicBool::new(false);
static LOG_FILE: AtomicBool = AtomicBool::new(false);

pub fn set_debug(v: bool) {
    DEBUG.store(v, Ordering::Relaxed);
}
pub fn set_log_file(v: bool) {
    LOG_FILE.store(v, Ordering::Relaxed);
}

fn append_session(line: &str) {
    if !Path::new(STATE_DIR).is_dir() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().append(true).create(true).mode(0o644).open(SESSION_LOG) {
        let _ = writeln!(f, "{}", line);
    }
}

fn emit(tag: &str, msg: &str) {
    for line in msg.lines() {
        let l = format!("[{}] {}", tag, line);
        println!("{}", l);
        if LOG_FILE.load(Ordering::Relaxed) {
            append_session(&l);
        }
    }
    let _ = std::io::stdout().flush();
}
pub fn debug(m: &str) {
    if DEBUG.load(Ordering::Relaxed) {
        emit("DEBUG", m)
    }
}
pub fn info(m: &str) {
    emit("INFO", m)
}
pub fn step(m: &str) {
    emit("STEP", m)
}
pub fn ok(m: &str) {
    emit("OK", m)
}
pub fn warn(m: &str) {
    emit("WARN", m)
}
pub fn error(m: &str) {
    emit("ERROR", m)
}

// ------------------------------------------------------------------ helpers
pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn find(tool: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in ["/usr/local/sbin", "/usr/sbin", "/sbin", "/usr/local/bin", "/usr/bin", "/bin"] {
        dirs.push(PathBuf::from(d));
    }
    for d in dirs {
        let p = d.join(tool);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}
pub fn have(tool: &str) -> bool {
    find(tool).is_some()
}

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
impl Out {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

fn clip(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= n {
        s.to_string()
    } else {
        format!("{} ...[{} more chars]", c[..n].iter().collect::<String>(), c.len() - n)
    }
}

pub fn tail(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    c[c.len().saturating_sub(n)..].iter().collect()
}

/// Run a command with timeout, capture output, log it in debug mode.
pub fn run(args: &[&str], timeout_s: u64, input: Option<&str>) -> Res<Out> {
    let exe = find(args[0]).ok_or_else(|| format!("missing tool: {}", args[0]))?;
    debug(&format!("$ {}", args.join(" ")));
    let mut child = Command::new(exe)
        .args(&args[1..])
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {}", args[0], e))?;
    if let Some(data) = input {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(data.as_bytes());
        }
    }
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let h1 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });
    let h2 = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });
    let deadline = Instant::now() + Duration::from_secs(timeout_s);
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break -2;
                }
                sleep(Duration::from_millis(20));
            }
            Err(_) => break -1,
        }
    };
    let stdout = h1.join().unwrap_or_default();
    let stderr = h2.join().unwrap_or_default();
    debug(&format!("  -> rc={}", code));
    if !stdout.trim().is_empty() {
        debug(&format!("  stdout: {}", clip(stdout.trim(), 3000)));
    }
    if !stderr.trim().is_empty() {
        debug(&format!("  stderr: {}", clip(stderr.trim(), 3000)));
    }
    Ok(Out { code, stdout, stderr })
}

pub fn sh(args: &[&str]) -> Out {
    run(args, 40, None).unwrap_or_else(|e| Out { code: 127, stdout: String::new(), stderr: e })
}
pub fn sh_t(args: &[&str], t: u64) -> Out {
    run(args, t, None).unwrap_or_else(|e| Out { code: 127, stdout: String::new(), stderr: e })
}
pub fn sh_check(args: &[&str]) -> Res<Out> {
    let o = run(args, 40, None)?;
    if o.code != 0 {
        let msg = if o.stderr.trim().is_empty() { &o.stdout } else { &o.stderr };
        return Err(format!("{} failed: {}", args.join(" "), msg.trim()));
    }
    Ok(o)
}

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}
fn cap(p: &str, t: &str) -> Option<String> {
    re(p).captures(t).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

pub fn pid_alive(pid: i32) -> bool {
    pid > 0 && Path::new(&format!("/proc/{}", pid)).exists()
}

pub fn kill_pid(pid: i32) {
    if !pid_alive(pid) {
        return;
    }
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..30 {
        if !pid_alive(pid) {
            return;
        }
        sleep(Duration::from_millis(100));
    }
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

// -------------------------------------------------------------------- state
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Config {
    pub ssid: String,
    pub password: String,
    pub iface: Option<String>,
    pub channel: Option<u32>,
    pub country: Option<String>,
    pub band: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct PinOrig {
    pub band: String,
    pub channel: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct State {
    pub wlan: String,
    pub ap: String,
    #[serde(rename = "virtual")]
    pub virt: bool,
    pub nm: bool,
    pub ssid: String,
    pub channel: u32,
    pub ipt_rules: Vec<(String, String, Vec<String>)>,
    pub fw: Option<String>,
    pub reconnect_conn: Option<String>,
    pub reconnect_needed: bool,
    pub pinned_conn: Option<String>,
    pub pin_orig: Option<PinOrig>,
    pub orig_forward: String,
    pub hostapd_pid: Option<i32>,
    pub dnsmasq_pid: Option<i32>,
    pub started: u64,
}

pub fn load_state() -> Option<State> {
    serde_json::from_str(&fs::read_to_string(STATE_FILE).ok()?).ok()
}

pub fn save_state(st: &State) -> Res<()> {
    fs::create_dir_all(STATE_DIR).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(STATE_DIR, fs::Permissions::from_mode(0o755));
    let tmp = format!("{}.tmp", STATE_FILE);
    fs::write(&tmp, serde_json::to_string(st).unwrap()).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644));
    fs::rename(&tmp, STATE_FILE).map_err(|e| e.to_string())
}

// ------------------------------------------------------------- wifi probing
pub fn wifi_ifaces() -> Vec<(String, String)> {
    let out = sh(&["iw", "dev"]).stdout;
    let (re_phy, re_if) = (re(r"^phy#(\d+)"), re(r"^Interface (\S+)"));
    let mut res = vec![];
    let mut phy = String::new();
    let mut cur: Option<String> = None;
    for line in out.lines() {
        let l = line.trim();
        if let Some(c) = re_phy.captures(l) {
            phy = format!("phy{}", &c[1]);
        }
        if let Some(c) = re_if.captures(l) {
            cur = Some(c[1].to_string());
        }
        if l.starts_with("type ") {
            if let Some(name) = cur.take() {
                if l == "type managed" && name != AP_IF {
                    res.push((name, phy.clone()));
                }
            }
        }
    }
    res
}

/// -> (ssid, channel, freq_mhz)
pub fn link_info(iface: &str) -> (Option<String>, Option<u32>, Option<u32>) {
    let link = sh(&["iw", "dev", iface, "link"]).stdout;
    if link.contains("Not connected") || link.trim().is_empty() {
        return (None, None, None);
    }
    let ssid = cap(r"SSID: (.*)", &link).map(|s| s.trim().to_string());
    let freq = cap(r"freq: (\d+)", &link).and_then(|s| s.parse().ok());
    let info = sh(&["iw", "dev", iface, "info"]).stdout;
    let ch = cap(r"channel (\d+)", &info).and_then(|s| s.parse().ok());
    (ssid, ch, freq)
}

pub fn default_uplink() -> Option<String> {
    cap(r"dev (\S+)", &sh(&["ip", "-4", "route", "show", "default"]).stdout)
}

/// -> (concurrent AP+client ok, single_channel_only)
pub fn supports_concurrent(phy: &str) -> (bool, bool) {
    let out = sh(&["iw", "phy", phy, "info"]).stdout;
    let key = "valid interface combinations:";
    if !out.contains(key) {
        return (false, true);
    }
    let sect = out.split(key).nth(1).unwrap_or("");
    let (r1, r2) = (re(r"^\t[A-Za-z]"), re(r"^\S"));
    let mut lines: Vec<&str> = vec![];
    for line in sect.lines() {
        if r1.is_match(line) || r2.is_match(line) {
            break;
        }
        lines.push(line);
    }
    let joined = lines.join(" ").replace("AP/VLAN", "");
    let (r_ap, r_tot, r_ch) = (re(r"\bAP\b"), re(r"total <= (\d+)"), re(r"#channels <= (\d+)"));
    for e in joined.split("* ") {
        if e.contains("managed") && r_ap.is_match(e) {
            let tot = r_tot.captures(e).and_then(|c| c[1].parse::<u32>().ok());
            let ch = r_ch.captures(e).and_then(|c| c[1].parse::<u32>().ok());
            if let Some(t) = tot {
                if t >= 2 {
                    return (true, ch.map(|c| c < 2).unwrap_or(false));
                }
            }
        }
    }
    (false, true)
}

pub fn reg_country() -> Option<String> {
    cap(r"country (\w\w):", &sh(&["iw", "reg", "get"]).stdout).filter(|c| c != "00")
}

pub fn saved_country() -> Option<String> {
    fs::read_to_string(COUNTRY_FILE)
        .ok()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| s.len() == 2)
}

/// Country from the system timezone (zone.tab), e.g. Asia/Kolkata -> IN.
pub fn guess_country() -> Option<String> {
    let real = fs::canonicalize("/etc/localtime").ok()?;
    let real = real.to_string_lossy().to_string();
    let tz = real.rsplit("zoneinfo/").next()?.to_string();
    for line in fs::read_to_string("/usr/share/zoneinfo/zone.tab").ok()?.lines() {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() >= 3 && p[2] == tz {
            return Some(p[0].to_string());
        }
    }
    None
}

pub fn band_of(freq: u32) -> &'static str {
    if freq < 3000 {
        "2.4"
    } else if freq < 5950 {
        "5"
    } else {
        "6"
    }
}
pub fn ch_band(ch: u32) -> &'static str {
    if ch <= 14 {
        "2.4"
    } else {
        "5"
    }
}

#[derive(Clone, Default)]
pub struct Channels {
    pub b24: Vec<u32>,
    pub b5: Vec<u32>,
}

/// Channels usable as AP right now (no disabled / no-IR / radar).
pub fn ap_channels(phy: &str) -> Channels {
    let out = sh(&["iw", "phy", phy, "info"]).stdout;
    let mut res = Channels::default();
    let r = re(r"\*\s+([\d.]+) MHz \[(\d+)\]([^\n]*)");
    for m in r.captures_iter(&out) {
        let freq = m[1].parse::<f64>().unwrap_or(0.0) as u32;
        let ch: u32 = m[2].parse().unwrap_or(0);
        let rest = &m[3];
        if ["disabled", "no IR", "radar"].iter().any(|k| rest.contains(k)) {
            continue;
        }
        match band_of(freq) {
            "2.4" => res.b24.push(ch),
            "5" => res.b5.push(ch),
            _ => {}
        }
    }
    res
}

/// split an nmcli -t line on unescaped ':'
fn split_nm(line: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                if let Some(n) = it.next() {
                    parts.last_mut().unwrap().push(n);
                }
            }
            ':' => parts.push(String::new()),
            _ => parts.last_mut().unwrap().push(c),
        }
    }
    parts
}

pub fn scan_channels(wlan: &str, ssid: &str) -> Vec<u32> {
    if !have("nmcli") || ssid.is_empty() {
        return vec![];
    }
    let out = sh_t(&["nmcli", "-t", "-f", "SSID,CHAN", "device", "wifi", "list", "ifname", wlan], 30).stdout;
    let mut chans = vec![];
    for line in out.lines() {
        let p = split_nm(line);
        if p.len() >= 2 && p[0] == ssid {
            if let Ok(c) = p[1].parse::<u32>() {
                if !chans.contains(&c) {
                    chans.push(c);
                }
            }
        }
    }
    chans
}

/// -> (ap_channel, pin_client_to_channel, note)
pub fn pick_channel(
    al: &Channels,
    pref: &str,
    want: Option<u32>,
    scan: &[u32],
    single: bool,
    conc: bool,
) -> (Option<u32>, Option<u32>, String) {
    let all: Vec<u32> = al.b24.iter().chain(al.b5.iter()).cloned().collect();
    if all.is_empty() {
        return (None, None, "this card/country allows no AP channels".into());
    }
    let in_pref = |c: u32| pref == "auto" || ch_band(c) == pref;
    let default_ap = |prefer: &str| -> Option<u32> {
        let mut order: Vec<&str> = vec![];
        if prefer == "2.4" || prefer == "5" {
            order.push(prefer);
        }
        order.push("2.4");
        order.push("5");
        for b in order {
            let list = if b == "2.4" { &al.b24 } else { &al.b5 };
            if !list.is_empty() {
                for c in [6, 1, 11, 36, 40, 44, 48, 149, 153, 157, 161] {
                    if list.contains(&c) {
                        return Some(c);
                    }
                }
                return Some(list[0]);
            }
        }
        None
    };
    let prefer = if pref == "auto" { "2.4" } else { pref };
    if conc && single {
        // AP and client must share ONE channel: use one the router offers.
        let mut cands: Vec<u32> = vec![];
        if let Some(w) = want {
            cands.push(w);
        }
        for &c in scan {
            if Some(c) != want {
                cands.push(c);
            }
        }
        for c in cands {
            if all.contains(&c) && in_pref(c) {
                return (Some(c), if Some(c) == want { None } else { Some(c) }, String::new());
            }
        }
        return (
            default_ap(prefer),
            None,
            "your router has no channel this card can host a hotspot on; Wi-Fi will not be reconnected".into(),
        );
    }
    if pref == "auto" {
        if let Some(w) = want {
            if all.contains(&w) {
                return (Some(w), None, String::new());
            }
        }
    }
    (default_ap(prefer), None, String::new())
}

pub fn nm_pin(conn: &str, ch: u32) -> PinOrig {
    let g = |k: &str| sh(&["nmcli", "-g", k, "connection", "show", conn]).stdout.trim().to_string();
    let orig = PinOrig { band: g("802-11-wireless.band"), channel: g("802-11-wireless.channel") };
    let chs = ch.to_string();
    sh(&["nmcli", "connection", "modify", conn, "802-11-wireless.band", if ch <= 14 { "bg" } else { "a" },
         "802-11-wireless.channel", &chs]);
    orig
}

pub fn nm_unpin(conn: &str, o: &PinOrig) {
    let ch = if o.channel.is_empty() { "0" } else { o.channel.as_str() };
    sh(&["nmcli", "connection", "modify", conn, "802-11-wireless.channel", ch, "802-11-wireless.band", &o.band]);
}

/// The world domain (00) blocks 5 GHz AP, so make sure a country is set.
pub fn ensure_country(preferred: Option<&str>) -> Option<String> {
    let cc = preferred
        .map(|s| s.to_uppercase())
        .or_else(reg_country)
        .or_else(saved_country)
        .or_else(guess_country);
    if let Some(c) = &cc {
        if reg_country().as_deref() != Some(c.as_str()) {
            sh(&["iw", "reg", "set", c]);
            sleep(Duration::from_millis(1500));
        }
    }
    reg_country().or(cc)
}

// ---------------------------------------------------------- network manager
pub fn nm_active() -> bool {
    have("nmcli") && sh(&["nmcli", "-t", "general", "status"]).ok()
}

pub fn nm_conn_on(iface: &str) -> Option<String> {
    let out = sh(&["nmcli", "-t", "-f", "NAME,TYPE,DEVICE", "connection", "show", "--active"]).stdout;
    for line in out.lines() {
        let p = split_nm(line);
        if p.len() >= 3 && p[2] == iface && p[1].contains("wireless") {
            return Some(p[0].clone());
        }
    }
    None
}

pub fn wifi_disconnect(iface: &str, nm: bool) {
    if nm {
        sh(&["nmcli", "device", "disconnect", iface]);
    } else if have("wpa_cli") {
        sh(&["wpa_cli", "-i", iface, "disconnect"]);
    }
}

pub fn wifi_reconnect(iface: &str, conn: Option<&str>, nm: bool) -> bool {
    if nm {
        if let Some(c) = conn {
            return sh_t(&["nmcli", "connection", "up", c, "ifname", iface], 60).ok();
        }
    }
    if have("wpa_cli") {
        sh(&["wpa_cli", "-i", iface, "reconnect"]);
        return true;
    }
    false
}

pub fn nm_set_managed(iface: &str, managed: bool) -> bool {
    for _ in 0..10 {
        if sh(&["nmcli", "device", "set", iface, "managed", if managed { "yes" } else { "no" }]).ok() {
            return true;
        }
        sleep(Duration::from_millis(300));
    }
    false
}

// ----------------------------------------------------------------- firewall
pub fn fw_up(ap: &str, st: &mut State) -> Res<()> {
    if have("iptables") {
        let ssub = format!("{}.0/24", SUBNET);
        let rules: Vec<(&str, &str, Vec<&str>)> = vec![
            ("filter", "INPUT", vec!["-i", ap, "-j", "ACCEPT"]),
            ("filter", "FORWARD", vec!["-i", ap, "-j", "ACCEPT"]),
            ("filter", "FORWARD", vec!["-o", ap, "-m", "conntrack", "--ctstate", "RELATED,ESTABLISHED", "-j", "ACCEPT"]),
            ("nat", "POSTROUTING", vec!["-s", ssub.as_str(), "!", "-o", ap, "-j", "MASQUERADE"]),
        ];
        for (t, c, a) in rules {
            let mut cmd = vec!["iptables", "-w", "-t", t, "-I", c, "1"];
            cmd.extend(a.iter().copied());
            sh_check(&cmd)?;
            st.ipt_rules.push((t.to_string(), c.to_string(), a.iter().map(|x| x.to_string()).collect()));
        }
        st.fw = Some("iptables".into());
        Ok(())
    } else if have("nft") {
        let script = r#"
table ip linspot_nat {
  chain post { type nat hook postrouting priority 100; policy accept;
    ip saddr @SUBNET@.0/24 oifname != "@AP@" masquerade }
}
table inet linspot {
  chain input { type filter hook input priority -10; policy accept; iifname "@AP@" accept }
  chain forward { type filter hook forward priority -10; policy accept;
    iifname "@AP@" accept
    oifname "@AP@" ct state established,related accept }
}
"#
        .replace("@SUBNET@", SUBNET)
        .replace("@AP@", ap);
        let o = run(&["nft", "-f", "-"], 40, Some(&script))?;
        if !o.ok() {
            return Err(format!("nft failed: {}", o.stderr.trim()));
        }
        st.fw = Some("nft".into());
        Ok(())
    } else {
        Err("need iptables or nft for internet sharing".into())
    }
}

pub fn fw_down(st: &State) {
    for (t, c, a) in st.ipt_rules.iter().rev() {
        let mut v = vec!["iptables", "-w", "-t", t.as_str(), "-D", c.as_str()];
        v.extend(a.iter().map(|x| x.as_str()));
        sh(&v);
    }
    if st.fw.as_deref() == Some("nft") {
        sh(&["nft", "delete", "table", "inet", "linspot"]);
        sh(&["nft", "delete", "table", "ip", "linspot_nat"]);
    }
}

// ------------------------------------------------------------------ hotspot
fn spawn_detached(exe: PathBuf, args: &[&str], logf: &str) -> Option<std::process::Child> {
    let f = fs::OpenOptions::new().append(true).create(true).open(logf).ok()?;
    let f2 = f.try_clone().ok()?;
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::from(f)).stderr(Stdio::from(f2));
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().ok()
}

pub fn launch_hostapd(conf: &str, logf: &str) -> Option<i32> {
    let _ = fs::write(logf, "");
    let exe = find("hostapd")?;
    debug(&format!("$ hostapd {}", conf));
    let mut child = spawn_detached(exe, &[conf], logf)?;
    for _ in 0..80 {
        sleep(Duration::from_millis(100));
        if let Ok(Some(_)) = child.try_wait() {
            return None;
        }
        if fs::read_to_string(logf).unwrap_or_default().contains("AP-ENABLED") {
            return Some(child.id() as i32);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

pub fn write_hostapd_conf(ap: &str, ssid: &str, pw: &str, channel: u32) -> Res<String> {
    let mut lines = vec![
        format!("interface={}", ap),
        "driver=nl80211".to_string(),
        format!("ssid={}", ssid),
        format!("hw_mode={}", if channel > 14 { "a" } else { "g" }),
        format!("channel={}", channel),
        "ieee80211n=1".into(),
        "wmm_enabled=1".into(),
        "auth_algs=1".into(),
        "wpa=2".into(),
        "wpa_key_mgmt=WPA-PSK".into(),
        "rsn_pairwise=CCMP".into(),
        format!("wpa_passphrase={}", pw),
        format!("ctrl_interface={}/hostapd", STATE_DIR),
        "ctrl_interface_group=0".into(),
    ];
    if let Some(c) = reg_country() {
        lines.push(format!("country_code={}", c));
        lines.push("ieee80211d=1".into());
    }
    if channel > 14 {
        lines.push("ieee80211ac=0".into());
    }
    let path = format!("{}/hostapd.conf", STATE_DIR);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all((lines.join("\n") + "\n").as_bytes()).map_err(|e| e.to_string())?;
    debug(&format!("hostapd.conf written ({} lines, password hidden)", lines.len()));
    Ok(path)
}

/// Create the virtual AP interface hs0 on the same radio.
/// `permanent_mac`: derive hs0's MAC from the adapter's permanent address (from `iw dev info`)
/// instead of wlan0's current (possibly NetworkManager-randomised) one.
pub fn make_virtual_if(wlan: &str, permanent_mac: bool) -> Res<()> {
    sh(&["iw", "dev", AP_IF, "del"]); // stale leftover, ignore errors
    sh_check(&["iw", "dev", wlan, "interface", "add", AP_IF, "type", "__ap"])?;
    let src = if permanent_mac {
        fs::read_to_string(format!("/sys/class/net/{}/phy80211/macaddress", wlan))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| s.len() == 17)
    } else {
        fs::read_to_string(format!("/sys/class/net/{}/address", wlan)).ok().map(|s| s.trim().to_string())
    };
    if let Some(m) = src {
        let parts: Vec<&str> = m.split(':').collect();
        if parts.len() == 6 {
            let mut b: Vec<u8> = parts.iter().map(|x| u8::from_str_radix(x, 16).unwrap_or(0)).collect();
            b[0] |= 0x02;
            b[5] = b[5].wrapping_add(1);
            let mac = b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(":");
            sh(&["ip", "link", "set", "dev", AP_IF, "address", &mac]);
        }
    }
    Ok(())
}

const NM_CONF_DIR: &str = "/etc/NetworkManager/conf.d";
const NM_CONF: &str = "/etc/NetworkManager/conf.d/90-linspot-unmanaged.conf";

/// NetworkManager grabs every new Wi-Fi interface and switches it to *station* mode, which makes
/// `ip link set hs0 up` fail with EBUSY on cards that allow only one station interface.
/// Tell NM once, persistently, to never touch hs0 (same idea as linux-wifi-hotspot).
pub fn ensure_nm_unmanaged(nm: bool) {
    if !nm || Path::new(NM_CONF).exists() || !Path::new(NM_CONF_DIR).is_dir() {
        return;
    }
    let body = format!("[keyfile]\nunmanaged-devices=interface-name:{}\n", AP_IF);
    if fs::write(NM_CONF, body).is_ok() {
        info(&format!("told NetworkManager to ignore {} ({})", AP_IF, NM_CONF));
        sh(&["nmcli", "general", "reload", "conf"]);
        sleep(Duration::from_millis(500));
    }
}

fn iface_type(ap: &str) -> String {
    cap(r"type (\S+)", &sh(&["iw", "dev", ap, "info"]).stdout).unwrap_or_default()
}

/// Make sure the virtual interface really is in AP mode (NM may have flipped it to managed).
fn ensure_ap_type(ap: &str) -> bool {
    let t = iface_type(ap);
    if t == "AP" {
        return true;
    }
    warn(&format!("{} is in '{}' mode (something switched it) - setting AP mode", ap, t));
    sh(&["ip", "link", "set", ap, "down"]);
    sh(&["iw", "dev", ap, "set", "type", "__ap"]);
    iface_type(ap) == "AP"
}

fn setup_addr(ap: &str) -> Res<()> {
    sh(&["ip", "addr", "flush", "dev", ap]);
    sh_check(&["ip", "addr", "add", &format!("{}/24", GW), "dev", ap])?;
    Ok(())
}

fn dump_kernel_hints() {
    let o = sh(&["dmesg"]);
    let lines: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| ["iwlwifi", "cfg80211", "wlan0", "hs0", "ath1", "mt76", "rtw", "brcm"].iter().any(|k| l.contains(k)))
        .collect();
    let n = lines.len().saturating_sub(10);
    for l in &lines[n..] {
        warn(&format!("kernel: {}", l));
    }
}

/// `ip link set <ap> up` can return EBUSY for a moment while the driver finishes tearing down
/// the station connection / a scan. Retry, and halfway through rebuild hs0 with another MAC.
fn bring_up_link(conc: bool, nm: bool, wlan: &str, ap: &str) -> Res<()> {
    let mut last = String::new();
    for attempt in 1..=14u32 {
        let o = sh(&["ip", "link", "set", ap, "up"]);
        if o.ok() {
            if attempt > 1 {
                info(&format!("{} is up (after {} attempts)", ap, attempt));
            }
            return Ok(());
        }
        last = if o.stderr.trim().is_empty() { o.stdout.trim().to_string() } else { o.stderr.trim().to_string() };
        if !last.to_lowercase().contains("busy") {
            break;
        }
        warn(&format!("{} is busy (attempt {}/14) - waiting for the Wi-Fi driver to settle ...", ap, attempt));
        if attempt == 1 {
            debug(&format!("iw dev:\n{}", sh(&["iw", "dev"]).stdout));
        }
        if conc {
            ensure_ap_type(ap);
        }
        sleep(Duration::from_millis(700));
        if conc && attempt == 7 {
            warn("recreating hs0 with the adapter's permanent MAC address");
            sh(&["ip", "addr", "flush", "dev", ap]);
            sh(&["iw", "dev", ap, "del"]);
            sleep(Duration::from_millis(500));
            make_virtual_if(wlan, true)?;
            if nm {
                nm_set_managed(ap, false);
            }
            ensure_ap_type(ap);
            setup_addr(ap)?;
        }
    }
    dump_kernel_hints();
    Err(format!("ip link set {} up failed: {}", ap, last))
}

fn prepare_ap(conc: bool, nm: bool, wlan: &str, ap: &str) -> Res<()> {
    if conc {
        make_virtual_if(wlan, false)?;
    }
    if nm {
        nm_set_managed(ap, false);
    }
    if conc {
        sleep(Duration::from_millis(300));
        ensure_ap_type(ap);
    }
    setup_addr(ap)?;
    bring_up_link(conc, nm, wlan, ap)
}

struct Plan {
    wlan: String,
    ssid_now: Option<String>,
    uplink: Option<String>,
    conc: bool,
    nm: bool,
    channel: u32,
    pin: Option<u32>,
    reconnect_ok: bool,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn start(cfg: &Config) -> Res<()> {
    if !is_root() {
        return Err("must run as root (use sudo or the GUI)".into());
    }
    let st0 = load_state();
    if let Some(s) = &st0 {
        if pid_alive(s.hostapd_pid.unwrap_or(0)) {
            return Err("hotspot is already running".into());
        }
    }
    fs::create_dir_all(STATE_DIR).map_err(|e| e.to_string())?;
    let _ = fs::set_permissions(STATE_DIR, fs::Permissions::from_mode(0o755));
    let _ = fs::write(SESSION_LOG, "");
    let _ = fs::set_permissions(SESSION_LOG, fs::Permissions::from_mode(0o644));
    let _ = fs::write(format!("{}/kicked", STATE_DIR), ""); // fresh session: show every device again
    if st0.is_some() {
        info("cleaning up stale hotspot state ...");
        stop(true)?;
    }

    let (ssid, pw) = (cfg.ssid.clone(), cfg.password.clone());
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("SSID must be 1-32 bytes".into());
    }
    if pw.len() < 8 || pw.len() > 63 {
        return Err("password must be 8-63 characters".into());
    }
    for t in ["iw", "hostapd", "dnsmasq", "ip"] {
        if !have(t) {
            return Err(format!("missing '{}' - run install.sh to install dependencies", t));
        }
    }
    sh(&["rfkill", "unblock", "wifi"]);
    let cc = ensure_country(cfg.country.as_deref());
    info(&format!("regulatory country: {}", cc.clone().unwrap_or_else(|| "unknown (set it in the Country tab)".into())));

    let ifs = wifi_ifaces();
    if ifs.is_empty() {
        return Err("no Wi-Fi adapter found".into());
    }
    let (wlan, phy) = match cfg.iface.as_deref().filter(|s| !s.is_empty()).and_then(|w| ifs.iter().find(|p| p.0 == w)) {
        Some(x) => x.clone(),
        None => ifs.iter().find(|p| link_info(&p.0).0.is_some()).cloned().unwrap_or_else(|| ifs[0].clone()),
    };

    let nm = nm_active();
    let (ssid_now, ch_now, _freq) = link_info(&wlan);
    let uplink = default_uplink();
    let (conc, single) = supports_concurrent(&phy);
    info(&format!("adapter {} ({}): concurrent AP+client {}", wlan, phy, if conc { "SUPPORTED" } else { "not supported" }));
    if conc && single {
        info("single-channel concurrency: AP and Wi-Fi client must share one channel");
    }

    let pref = cfg.band.clone().unwrap_or_else(|| "auto".into());
    let pref = if pref.is_empty() { "auto".to_string() } else { pref };
    let allowed = ap_channels(&phy);
    let show = |v: &Vec<u32>| if v.is_empty() { "none".to_string() } else { format!("{:?}", v) };
    info(&format!("AP-capable channels  2.4GHz: {}   5GHz: {}", show(&allowed.b24), show(&allowed.b5)));
    let scan = match &ssid_now {
        Some(s) => scan_channels(&wlan, s),
        None => vec![],
    };
    let mut channel = cfg.channel.unwrap_or(0);
    let mut pin = None;
    let mut reconnect_ok = true;
    if channel == 0 {
        let (c, p, note) = pick_channel(&allowed, &pref, ch_now, &scan, single, conc);
        pin = p;
        if !note.is_empty() {
            warn(&note);
            reconnect_ok = false;
        }
        channel = c.ok_or("no usable hotspot channel: set the country or pick another band")?;
    }
    info(&format!(
        "hotspot channel {} ({} GHz){}",
        channel,
        ch_band(channel),
        if pin.is_some() { ", Wi-Fi will be moved to this channel" } else { "" }
    ));

    let ap = if conc { AP_IF.to_string() } else { wlan.clone() };
    let mut st = State {
        wlan: wlan.clone(),
        ap,
        virt: conc,
        nm,
        ssid: ssid.clone(),
        channel,
        orig_forward: fs::read_to_string("/proc/sys/net/ipv4/ip_forward").unwrap_or_else(|_| "0".into()).trim().to_string(),
        started: now_secs(),
        ..Default::default()
    };
    save_state(&st)?;
    let plan = Plan { wlan, ssid_now, uplink, conc, nm, channel, pin, reconnect_ok };
    let r = bring_up(cfg, &mut st, &plan);
    if r.is_err() {
        warn("start failed - rolling back");
        let _ = stop(true);
    }
    r
}

fn bring_up(cfg: &Config, st: &mut State, p: &Plan) -> Res<()> {
    let ap = st.ap.clone();
    let wlan = p.wlan.clone();
    if !p.conc && p.uplink.as_deref() == Some(wlan.as_str()) {
        warn("this card cannot share and stay connected at once; the hotspot will have no internet (use Ethernet/USB tether as uplink).");
    }
    let conn = if p.nm && p.ssid_now.is_some() { nm_conn_on(&wlan) } else { None };
    if p.conc {
        ensure_nm_unmanaged(p.nm);
    }
    let conf = write_hostapd_conf(&ap, &cfg.ssid, &cfg.password, p.channel)?;
    info(&format!("starting access point on channel {} ...", p.channel));

    // Flow: 1) disconnect current Wi-Fi  2) hotspot ON  3) reconnect same Wi-Fi
    if let Some(sn) = &p.ssid_now {
        step(&format!("1/3 disconnecting Wi-Fi '{}' ...", sn));
        st.reconnect_conn = conn.clone();
        st.reconnect_needed = true;
        if let (Some(pc), Some(c), true) = (p.pin, conn.as_ref(), p.nm) {
            st.pinned_conn = Some(c.clone());
            st.pin_orig = Some(nm_pin(c, pc));
        }
        save_state(st)?;
        wifi_disconnect(&wlan, p.nm);
        for _ in 0..40 {
            // wait (max 8 s) until the driver reports "Not connected"
            if link_info(&wlan).0.is_none() {
                break;
            }
            sleep(Duration::from_millis(200));
        }
        sleep(Duration::from_millis(1500));
    }
    step("2/3 turning hotspot on ...");
    prepare_ap(p.conc, p.nm, &wlan, &ap)?;
    let hlog = format!("{}/hostapd.log", STATE_DIR);
    let pid = launch_hostapd(&conf, &hlog);
    if pid.is_some() && p.ssid_now.is_some() && p.conc && !p.reconnect_ok {
        st.reconnect_needed = true; // restored on OFF
    } else if pid.is_some() && p.ssid_now.is_some() && p.conc {
        step(&format!("3/3 reconnecting to Wi-Fi '{}' ...", p.ssid_now.clone().unwrap_or_default()));
        let ok_ = wifi_reconnect(&wlan, conn.as_deref(), p.nm);
        if ok_ {
            ok("Wi-Fi reconnected");
        } else {
            warn("could not auto-reconnect Wi-Fi (connect manually)");
        }
        st.reconnect_needed = false;
    } else if pid.is_some() && p.ssid_now.is_some() && !p.conc {
        st.reconnect_needed = true; // restored when you press OFF
    }
    let pid = match pid {
        Some(x) => x,
        None => {
            let t = fs::read_to_string(&hlog).unwrap_or_default();
            let t = tail(&t, 400);
            let mut hint = String::new();
            if t.contains("support configured channel") {
                hint = format!(
                    "\n\nHint: channel {} is not allowed for an AP on this card/country. Set your country in the Country tab, \
or connect your Wi-Fi to a 2.4 GHz network / lower 5 GHz channel (36-48) and retry.",
                    p.channel
                );
            }
            return Err(format!("hostapd failed to start:\n{}{}", t, hint));
        }
    };
    st.hostapd_pid = Some(pid);
    save_state(st)?;
    ok(&format!("hostapd running (pid {})", pid));

    let dlog = format!("{}/dnsmasq.log", STATE_DIR);
    let _ = fs::write(&dlog, "");
    let range = format!("--dhcp-range={s}.10,{s}.250,255.255.255.0,12h", s = SUBNET);
    let ifarg = format!("--interface={}", ap);
    let listen = format!("--listen-address={}", GW);
    let opt3 = format!("--dhcp-option=3,{}", GW);
    let opt6 = format!("--dhcp-option=6,{}", GW);
    let lease = format!("--dhcp-leasefile={}/leases", STATE_DIR);
    let dargs = [
        "-k", "--conf-file=/dev/null", "--no-resolv", "--no-hosts", ifarg.as_str(), listen.as_str(),
        "--bind-interfaces", "--except-interface=lo", "--dhcp-authoritative", range.as_str(), opt3.as_str(),
        opt6.as_str(), "--server=1.1.1.1", "--server=9.9.9.9", lease.as_str(),
    ];
    debug(&format!("$ dnsmasq {}", dargs.join(" ")));
    let dexe = find("dnsmasq").ok_or("missing dnsmasq")?;
    let mut dns = spawn_detached(dexe, &dargs, &dlog).ok_or("could not launch dnsmasq")?;
    sleep(Duration::from_millis(800));
    if let Ok(Some(_)) = dns.try_wait() {
        return Err(format!("dnsmasq failed: {}", tail(&fs::read_to_string(&dlog).unwrap_or_default(), 300)));
    }
    st.dnsmasq_pid = Some(dns.id() as i32);
    ok(&format!("dnsmasq running (pid {})", dns.id()));

    fs::write("/proc/sys/net/ipv4/ip_forward", "1").map_err(|e| format!("cannot enable ip_forward: {}", e))?;
    fw_up(&ap, st)?;
    ok(&format!("NAT enabled via {}", st.fw.clone().unwrap_or_default()));
    save_state(st)?;
    ok(&format!("HOTSPOT ON  ssid={}  gateway={}", cfg.ssid, GW));
    Ok(())
}

pub fn stop(quiet: bool) -> Res<()> {
    if !is_root() {
        return Err("must run as root (use sudo or the GUI)".into());
    }
    let st = match load_state() {
        Some(s) => s,
        None => {
            if !quiet {
                info("hotspot not running");
            }
            return Ok(());
        }
    };
    if !quiet {
        step("stopping hotspot ...");
    }
    for p in [st.dnsmasq_pid, st.hostapd_pid].into_iter().flatten() {
        kill_pid(p);
    }
    fw_down(&st);
    let fwd = if st.orig_forward.is_empty() { "0" } else { st.orig_forward.as_str() };
    let _ = fs::write("/proc/sys/net/ipv4/ip_forward", fwd);
    let (wlan, ap) = (st.wlan.clone(), st.ap.clone());
    sh(&["ip", "addr", "flush", "dev", &ap]);
    if st.virt {
        sh(&["iw", "dev", &ap, "del"]);
    } else {
        sh(&["iw", "dev", &wlan, "set", "type", "managed"]);
        sh(&["ip", "link", "set", &wlan, "up"]);
    }
    if st.nm && !st.virt {
        nm_set_managed(&wlan, true);
    }
    if st.reconnect_needed {
        sleep(Duration::from_secs(1));
        let ok_ = wifi_reconnect(&wlan, st.reconnect_conn.as_deref(), st.nm);
        if !quiet {
            if ok_ {
                ok("Wi-Fi restored");
            } else {
                warn("could not auto-reconnect Wi-Fi");
            }
        }
    }
    if let (Some(c), Some(o)) = (&st.pinned_conn, &st.pin_orig) {
        nm_unpin(c, o); // put the saved connection's band/channel back
    }
    let _ = fs::remove_file(STATE_FILE);
    if !quiet {
        ok("HOTSPOT OFF");
    }
    Ok(())
}

// ------------------------------------------------------------ status/clients
#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub running: bool,
    pub ssid: String,
    pub channel: u32,
    pub clients: usize,
    pub concurrent: bool,
    pub started: u64,
    /// connected devices, already filtered by kicks (empty when the hotspot is off)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub client_list: Vec<ClientInfo>,
}

#[derive(Serialize, Clone)]
pub struct ClientInfo {
    pub ip: String,
    pub mac: String,
    pub host: String,
}

#[derive(Clone)]
pub struct Client {
    pub ip: String,
    pub mac: String,
    pub host: String,
    pub expires: u64,
}

pub fn leases() -> Vec<Client> {
    let now = now_secs();
    // MACs disconnected with `kick`, with the lease expiry they had when they were kicked:
    // as long as dnsmasq keeps handing out the *same* lease (device offline), these stay hidden;
    // a fresh lease for a reconnected device has a new expiry and shows up again.
    let kicked: Vec<(String, u64)> = fs::read_to_string(format!("{}/kicked", STATE_DIR))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            if p.len() == 2 {
                Some((p[0].to_lowercase(), p[1].parse().unwrap_or(0)))
            } else {
                None
            }
        })
        .collect();
    let mut v = vec![];
    for l in fs::read_to_string(format!("{}/leases", STATE_DIR)).unwrap_or_default().lines() {
        let p: Vec<&str> = l.split_whitespace().collect();
        if p.len() >= 4 {
            let exp: u64 = p[0].parse().unwrap_or(0);
            if exp == 0 || exp > now {
                let mac = p[1].to_lowercase();
                if kicked.iter().any(|(m, e)| *m == mac && (*e == exp || *e == 0)) {
                    continue;
                }
                v.push(Client { ip: p[2].into(), mac: p[1].into(), host: if p[3] == "*" { "-".into() } else { p[3].into() }, expires: exp });
            }
        }
    }
    v
}

pub fn status() -> Status {
    match load_state() {
        Some(st) if pid_alive(st.hostapd_pid.unwrap_or(0)) => {
            let ls = leases();
            Status {
                running: true,
                ssid: st.ssid,
                channel: st.channel,
                clients: ls.len(),
                concurrent: st.virt,
                started: st.started,
                client_list: ls
                    .iter()
                    .map(|c| ClientInfo { ip: c.ip.clone(), mac: c.mac.clone(), host: c.host.clone() })
                    .collect(),
            }
        }
        _ => Status::default(),
    }
}

// ------------------------------------------------------------------ client control
/// Drop the device's DHCP lease so every UI stops listing it right away, and remember the
/// kick in a world-readable file: if dnsmasq later rewrites the lease file with the same
/// (unchanged) lease, `leases()` keeps it hidden - until the device truly rejoins and
/// dnsmasq hands out a fresh lease with a new expiry.
fn hide_lease(mac: &str) {
    let path = format!("{}/leases", STATE_DIR);
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<&str> = vec![];
    let mut expiries: Vec<u64> = vec![];
    for l in text.lines() {
        let p: Vec<&str> = l.split_whitespace().collect();
        if p.len() >= 4 && p[1].eq_ignore_ascii_case(mac) {
            expiries.push(p[0].parse().unwrap_or(0));
        } else {
            kept.push(l);
        }
    }
    let joined = if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" };
    if fs::write(&path, joined).is_ok() {
        debug(&format!("removed {}'s lease from {}", mac, path));
    }
    if !expiries.is_empty() {
        let kp = format!("{}/kicked", STATE_DIR);
        let mut lines: Vec<String> = fs::read_to_string(&kp).unwrap_or_default().lines().map(|s| s.to_string()).collect();
        for e in expiries {
            let entry = format!("{} {}", mac, e);
            if !lines.contains(&entry) {
                lines.push(entry);
            }
        }
        if fs::write(&kp, lines.join("\n") + "\n").is_ok() {
            let _ = fs::set_permissions(&kp, fs::Permissions::from_mode(0o644));
        }
    }
}

/// Disconnect (deauthenticate) one client through the hostapd control interface.
pub fn kick(mac: &str) -> Res<()> {
    if !is_root() {
        return Err("must run as root (use sudo or the GUI)".into());
    }
    let st = match load_state() {
        Some(s) if pid_alive(s.hostapd_pid.unwrap_or(0)) => s,
        _ => return Err("hotspot is not running".into()),
    };
    let mac = mac.trim().to_lowercase();
    if !Regex::new(r"^([0-9a-f]{2}:){5}[0-9a-f]{2}$").unwrap().is_match(&mac) {
        return Err(format!("'{}' is not a valid MAC address (aa:bb:cc:dd:ee:ff)", mac));
    }
    if !have("hostapd_cli") {
        return Err("missing 'hostapd_cli' (shipped with hostapd on most distros)".into());
    }
    step(&format!("disconnecting device {} ...", mac));
    let ctrl = format!("{}/hostapd", STATE_DIR);
    let o = sh_t(&["hostapd_cli", "-p", &ctrl, "-i", &st.ap, "DEAUTHENTICATE", &mac], 15);
    if o.ok() {
        hide_lease(&mac);
        ok(&format!("device {} disconnected (it may reconnect if it wants to)", mac));
        Ok(())
    } else {
        Err(format!("could not disconnect {}: {}", mac, o.stderr.trim()))
    }
}

// ------------------------------------------------------------------ country
fn set_kv_file(path: &str, key: &str, val: &str, quoted: bool) -> std::io::Result<()> {
    let old = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| !l.trim_start().starts_with(&format!("{}=", key)))
        .map(|s| s.to_string())
        .collect();
    lines.push(if quoted { format!("{}=\"{}\"", key, val) } else { format!("{}={}", key, val) });
    fs::write(path, lines.join("\n") + "\n")
}

/// Set the Wi-Fi regulatory country now, and optionally make it permanent.
pub fn set_country(cc: &str, persist: bool) -> Res<()> {
    let cc = cc.trim().to_uppercase();
    if cc.len() != 2 || !cc.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("country must be a 2-letter ISO code, e.g. IN, US, DE".into());
    }
    if !is_root() {
        return Err("must run as root (use sudo or the GUI)".into());
    }
    if !have("iw") {
        return Err("missing 'iw'".into());
    }
    step(&format!("setting regulatory domain to {} ...", cc));
    sh_check(&["iw", "reg", "set", &cc])?;
    sleep(Duration::from_millis(1200));
    let now = reg_country();
    if now.as_deref() == Some(cc.as_str()) {
        ok(&format!("regulatory domain is now {}", cc));
    } else {
        warn(&format!(
            "kernel reports {:?} (self-managed adapters or missing wireless-regdb may ignore this)",
            now
        ));
    }
    if persist {
        let _ = fs::create_dir_all("/etc/linspot");
        match fs::write(COUNTRY_FILE, format!("{}\n", cc)) {
            Ok(_) => info(&format!("saved to {} (applied on every hotspot start)", COUNTRY_FILE)),
            Err(e) => warn(&format!("could not write {}: {}", COUNTRY_FILE, e)),
        }
        let crda = "/etc/default/crda";
        if Path::new(crda).exists() || (Path::new("/etc/default").is_dir() && !Path::new("/etc/conf.d").is_dir()) {
            match set_kv_file(crda, "REGDOMAIN", &cc, false) {
                Ok(_) => info(&format!("updated {} (REGDOMAIN={})", crda, cc)),
                Err(e) => warn(&format!("{}: {}", crda, e)),
            }
        }
        let arch = "/etc/conf.d/wireless-regdom";
        if Path::new(arch).exists() {
            match set_kv_file(arch, "WIRELESS_REGDOM", &cc, true) {
                Ok(_) => info(&format!("updated {} (WIRELESS_REGDOM=\"{}\")", arch, cc)),
                Err(e) => warn(&format!("{}: {}", arch, e)),
            }
        }
        ok("country saved permanently");
    }
    Ok(())
}

// -------------------------------------------------------------- system info
pub struct ToolRow {
    pub name: &'static str,
    pub purpose: &'static str,
    pub required: bool,
    pub found: bool,
}

pub fn tools_status() -> Vec<ToolRow> {
    let t = |name, purpose, required, found| ToolRow { name, purpose, required, found };
    vec![
        t("iw", "Wi-Fi capabilities, virtual AP interface, regulatory domain", true, have("iw")),
        t("hostapd", "Access point daemon", true, have("hostapd")),
        t("dnsmasq", "DHCP + DNS for hotspot clients", true, have("dnsmasq")),
        t("ip", "Interface / address configuration (iproute2)", true, have("ip")),
        t("iptables / nft", "NAT (internet sharing)", true, have("iptables") || have("nft")),
        t("pkexec", "Administrator prompt used by the GUI (polkit)", true, have("pkexec")),
        t("nmcli", "NetworkManager: disconnect / reconnect your Wi-Fi", false, have("nmcli")),
        t("wpa_cli", "wpa_supplicant fallback when no NetworkManager", false, have("wpa_cli")),
        t("rfkill", "Unblock Wi-Fi radio", false, have("rfkill")),
    ]
}

pub fn os_release() -> (String, String) {
    let txt = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let get = |k: &str| {
        txt.lines()
            .find_map(|l| l.strip_prefix(&format!("{}=", k)))
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default()
    };
    (get("ID"), get("PRETTY_NAME"))
}

pub fn pkg_manager() -> &'static str {
    for (bin, name) in [
        ("pacman", "pacman"), ("apt-get", "apt"), ("dnf", "dnf"), ("zypper", "zypper"),
        ("apk", "apk"), ("xbps-install", "xbps"), ("emerge", "portage"),
    ] {
        if have(bin) {
            return name;
        }
    }
    "unknown"
}

#[derive(Clone, Default)]
pub struct Adapter {
    pub iface: String,
    pub phy: String,
    pub ssid: Option<String>,
    pub channel: Option<u32>,
    pub concurrent: bool,
    pub single: bool,
    pub ch24: Vec<u32>,
    pub ch5: Vec<u32>,
}

pub fn adapters() -> Vec<Adapter> {
    wifi_ifaces()
        .into_iter()
        .map(|(iface, phy)| {
            let (ssid, channel, _) = link_info(&iface);
            let (concurrent, single) = supports_concurrent(&phy);
            let ch = ap_channels(&phy);
            Adapter { iface, phy, ssid, channel, concurrent, single, ch24: ch.b24, ch5: ch.b5 }
        })
        .collect()
}

#[derive(Default)]
pub struct SysInfo {
    pub adapters: Vec<Adapter>,
    pub tools: Vec<ToolRow>,
    pub reg: Option<String>,
    pub guess: Option<String>,
    pub saved: Option<String>,
    pub distro: String,
    pub pm: String,
}

pub fn sysinfo() -> SysInfo {
    let (id, pretty) = os_release();
    SysInfo {
        adapters: if have("iw") { adapters() } else { vec![] },
        tools: tools_status(),
        reg: if have("iw") { reg_country() } else { None },
        guess: guess_country(),
        saved: saved_country(),
        distro: if pretty.is_empty() { id } else { pretty },
        pm: pkg_manager().to_string(),
    }
}

/// Detailed, unprivileged diagnostics dump (lines already level-tagged).
pub fn diagnostics() -> Vec<String> {
    let mut v: Vec<String> = vec![];
    let (id, pretty) = os_release();
    v.push(format!("[INFO] ===== diagnostics ({} / {}) =====", pretty, id));
    let mut block = |title: &str, args: &[&str]| {
        v.push(format!("[STEP] {}", title));
        let o = sh(args);
        let text = format!("{}{}", o.stdout, o.stderr);
        if o.code == 127 {
            v.push(format!("[WARN] {} not available", args[0]));
        }
        for l in text.lines().take(60) {
            v.push(format!("[DEBUG] {}", l));
        }
    };
    block("kernel", &["uname", "-sr"]);
    block("wireless interfaces", &["iw", "dev"]);
    block("regulatory domain", &["iw", "reg", "get"]);
    block("rfkill", &["rfkill", "list"]);
    block("addresses", &["ip", "-br", "addr"]);
    block("default route", &["ip", "route", "show", "default"]);
    block("NetworkManager", &["nmcli", "-t", "general", "status"]);
    block("NM devices", &["nmcli", "-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]);
    block("hostapd version", &["hostapd", "-v"]);
    block("dnsmasq version", &["dnsmasq", "--version"]);
    for (i, p) in wifi_ifaces() {
        let (c, s) = supports_concurrent(&p);
        let ch = ap_channels(&p);
        v.push(format!(
            "[INFO] {} ({}): concurrent={} single-channel={} AP 2.4GHz={:?} 5GHz={:?}",
            i, p, c, s, ch.b24, ch.b5
        ));
    }
    v.push("[OK] diagnostics complete".into());
    v
}
