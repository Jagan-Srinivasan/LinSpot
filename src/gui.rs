//! egui front-end. The GUI never runs as root: privileged work is done by re-invoking
//! this same binary through pkexec ("linspot start --stdin", "linspot stop", ...) and
//! streaming its tagged output into the log view.
use crate::backend::{self, Adapter, Client, Status, SysInfo};
use crate::countries::COUNTRIES;
use crate::qr;
use crate::widgets::{self, Detect, Target, TargetInfo};
use eframe::egui::{self, Color32, Margin, RichText, Rounding, Stroke, TextStyle, Vec2};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(14, 17, 23);
const PANEL: Color32 = Color32::from_rgb(20, 24, 32);
const CARD: Color32 = Color32::from_rgb(27, 32, 42);
const BORDER: Color32 = Color32::from_rgb(44, 51, 66);
const ACCENT: Color32 = Color32::from_rgb(56, 189, 248);
const GREEN: Color32 = Color32::from_rgb(52, 211, 153);
const RED: Color32 = Color32::from_rgb(248, 113, 113);
const AMBER: Color32 = Color32::from_rgb(251, 191, 36);
const DIM: Color32 = Color32::from_rgb(140, 150, 170);
const TEXT: Color32 = Color32::from_rgb(226, 232, 240);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Hotspot,
    Clients,
    Share,
    Country,
    Logs,
    Widgets,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Debug,
    Info,
    Step,
    Ok,
    Warn,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogSrc {
    Live,
    Session,
    Hostapd,
    Dnsmasq,
}
impl LogSrc {
    const ALL: [LogSrc; 4] = [LogSrc::Live, LogSrc::Session, LogSrc::Hostapd, LogSrc::Dnsmasq];
    fn label(self) -> &'static str {
        match self {
            LogSrc::Live => "This GUI session",
            LogSrc::Session => "Backend session.log",
            LogSrc::Hostapd => "hostapd.log",
            LogSrc::Dnsmasq => "dnsmasq.log",
        }
    }
    fn path(self) -> Option<String> {
        match self {
            LogSrc::Live => None,
            LogSrc::Session => Some(backend::SESSION_LOG.to_string()),
            LogSrc::Hostapd => Some(format!("{}/hostapd.log", backend::STATE_DIR)),
            LogSrc::Dnsmasq => Some(format!("{}/dnsmasq.log", backend::STATE_DIR)),
        }
    }
}

#[derive(Clone)]
struct LogLine {
    ts: String,
    level: Level,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Start,
    Stop,
    Kick,
    Country,
    WidgetDeps,
    WidgetInstall,
    WidgetRemove,
}

enum Msg {
    Line(String, bool),
    Done(Job, i32),
    Sys(Box<SysInfo>),
    Diag(Vec<String>),
    Widgets(Box<Detect>, Vec<TargetInfo>),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct GuiConfig {
    ssid: String,
    password: String,
    band: String,
    channel: u32,
    iface: String,
    debug: bool,
}
impl Default for GuiConfig {
    fn default() -> Self {
        GuiConfig { ssid: "LinSpot".into(), password: String::new(), band: "auto".into(), channel: 0, iface: String::new(), debug: true }
    }
}

fn long_ago() -> Instant {
    Instant::now().checked_sub(Duration::from_secs(5)).unwrap_or_else(Instant::now)
}

pub fn cfg_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("linspot").join("config.json")
}

/// SSID + password the GUI last saved (used by `linspot qr` and the widgets).
pub fn saved_credentials() -> Option<(String, String)> {
    let c = load_cfg();
    if c.ssid.is_empty() || c.password.is_empty() {
        None
    } else {
        Some((c.ssid, c.password))
    }
}

fn load_cfg() -> GuiConfig {
    std::fs::read_to_string(cfg_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_cfg(c: &GuiConfig) {
    use std::os::unix::fs::OpenOptionsExt;
    let p = cfg_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&p) {
        let _ = f.write_all(serde_json::to_string_pretty(c).unwrap_or_default().as_bytes());
    }
}

fn now_hms() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

fn parse_line(raw: &str, from_err: bool) -> (Level, String) {
    if let Some(rest) = raw.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let lv = match &rest[..end] {
                "DEBUG" => Some(Level::Debug),
                "INFO" => Some(Level::Info),
                "STEP" => Some(Level::Step),
                "OK" => Some(Level::Ok),
                "WARN" => Some(Level::Warn),
                "ERROR" => Some(Level::Error),
                _ => None,
            };
            if let Some(l) = lv {
                return (l, rest[end + 1..].trim_start().to_string());
            }
        }
    }
    if let Some(r) = raw.strip_prefix("ERROR:") {
        return (Level::Error, r.trim().to_string());
    }
    (if from_err { Level::Warn } else { Level::Info }, raw.to_string())
}

fn guess_level(text: &str) -> Level {
    let t = text.to_lowercase();
    if t.contains("error") || t.contains("fail") {
        Level::Error
    } else if t.contains("warn") {
        Level::Warn
    } else if t.contains("ap-enabled") || t.contains("ap-sta-connected") {
        Level::Ok
    } else {
        Level::Info
    }
}

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Debug => DIM,
        Level::Info => TEXT,
        Level::Step => ACCENT,
        Level::Ok => GREEN,
        Level::Warn => AMBER,
        Level::Error => RED,
    }
}
fn level_tag(l: Level) -> &'static str {
    match l {
        Level::Debug => "DEBUG",
        Level::Info => "INFO ",
        Level::Step => "STEP ",
        Level::Ok => "OK   ",
        Level::Warn => "WARN ",
        Level::Error => "ERROR",
    }
}
fn lvl_ok(l: Level, f: (bool, bool, bool, bool)) -> bool {
    match l {
        Level::Debug => f.0,
        Level::Info | Level::Step | Level::Ok => f.1,
        Level::Warn => f.2,
        Level::Error => f.3,
    }
}

fn uptime_str(started: u64) -> String {
    if started == 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let s = now.saturating_sub(started);
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{}h {:02}m", h, m)
    } else {
        format!("{}m {:02}s", m, s % 60)
    }
}

fn random_password() -> String {
    use std::io::Read;
    const CS: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut b = [0u8; 12];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b.iter().map(|x| CS[*x as usize % CS.len()] as char).collect()
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).strong().size(16.0).color(TEXT));
                ui.add_space(8.0);
            }
            add(ui);
        });
    ui.add_space(10.0);
}

fn pill(ui: &mut egui::Ui, text: &str, c: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 38))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(12.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(c).strong());
        });
}

fn dot(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 5.0, c);
}

fn setup_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(10, 12, 17);
    v.faint_bg_color = Color32::from_rgb(24, 29, 38);
    v.selection.bg_fill = Color32::from_rgb(14, 116, 160);
    v.widgets.inactive.bg_fill = Color32::from_rgb(36, 43, 56);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 43, 56);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 57, 74);
    v.widgets.hovered.bg_fill = Color32::from_rgb(48, 57, 74);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive] {
        w.rounding = Rounding::same(10.0);
    }
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 9.0);
        s.spacing.button_padding = egui::vec2(14.0, 7.0);
        s.text_styles.insert(TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
}

enum Act {
    Copy(String),
    Save(String),
    Clear,
    Diag,
}

struct App {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    tab: Tab,
    cfg: GuiConfig,
    show_pw: bool,
    status: Status,
    clients: Vec<Client>,
    last_poll: Instant,
    busy: Option<Job>,
    progress: String,
    footer: (String, Color32),
    live: Vec<LogLine>,
    file_lines: Vec<LogLine>,
    file_poll: Instant,
    log_src: LogSrc,
    f_debug: bool,
    f_info: bool,
    f_warn: bool,
    f_error: bool,
    search: String,
    autoscroll: bool,
    show_ts: bool,
    sys: SysInfo,
    sys_loading: bool,
    country_sel: String,
    country_filter: String,
    country_persist: bool,
    wdet: Detect,
    winfo: Vec<TargetInfo>,
    w_loading: bool,
    w_tip_shown: bool,
    wpending: Option<Target>,
    qr: Option<Result<qr::Qr, String>>,
    qr_key: (String, String),
}

pub fn run() -> i32 {
    if backend::is_root() {
        eprintln!("note: running the GUI as root is not recommended; run it as your normal user.");
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 690.0])
            .with_min_inner_size([840.0, 560.0])
            .with_title("LinSpot")
            .with_app_id("linspot"),
        // vsync=true makes eglSwapBuffers block whenever the compositor stops sending frame
        // callbacks (window on another workspace, minimised, occluded, idle). The blocked event
        // loop then cannot answer xdg_wm_base pings and Hyprland/KWin/GNOME show
        // "application not responding". Without vsync the loop never blocks on the compositor.
        vsync: false,
        ..Default::default()
    };
    match eframe::run_native("LinSpot", opts, Box::new(|cc| Ok(Box::new(App::new(cc))))) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("GUI error: {}", e);
            1
        }
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = App {
            tx,
            rx,
            tab: Tab::Hotspot,
            cfg: load_cfg(),
            show_pw: false,
            status: Status::default(),
            clients: vec![],
            last_poll: long_ago(),
            busy: None,
            progress: String::new(),
            footer: ("Ready".into(), DIM),
            live: vec![],
            file_lines: vec![],
            file_poll: long_ago(),
            log_src: LogSrc::Live,
            f_debug: true,
            f_info: true,
            f_warn: true,
            f_error: true,
            search: String::new(),
            autoscroll: true,
            show_ts: true,
            sys: SysInfo::default(),
            sys_loading: false,
            country_sel: String::new(),
            country_filter: String::new(),
            country_persist: true,
            wdet: Detect::default(),
            winfo: vec![],
            w_loading: false,
            w_tip_shown: false,
            wpending: None,
            qr: None,
            qr_key: (String::new(), String::new()),
        };
        app.add_line(Level::Info, "LinSpot started".into());
        app.refresh_sys(&cc.egui_ctx);
        app.refresh_widgets(&cc.egui_ctx);
        app
    }

    fn add_line(&mut self, level: Level, text: String) {
        self.live.push(LogLine { ts: now_hms(), level, text });
        if self.live.len() > 20000 {
            self.live.drain(0..5000);
        }
    }

    fn refresh_sys(&mut self, ctx: &egui::Context) {
        if self.sys_loading {
            return;
        }
        self.sys_loading = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(Msg::Sys(Box::new(backend::sysinfo())));
            ctx.request_repaint();
        });
    }

    fn run_diag(&mut self, ctx: &egui::Context) {
        self.add_line(Level::Step, "collecting diagnostics ...".into());
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(Msg::Diag(backend::diagnostics()));
            ctx.request_repaint();
        });
    }

    fn run_job(&mut self, ctx: &egui::Context, job: Job, args: Vec<String>, stdin_data: Option<String>) {
        self.run_job_ex(ctx, job, args, stdin_data, true);
    }

    fn refresh_widgets(&mut self, ctx: &egui::Context) {
        if self.w_loading {
            return;
        }
        self.w_loading = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            let (d, i) = widgets::scan();
            let _ = tx.send(Msg::Widgets(Box::new(d), i));
            ctx.request_repaint();
        });
    }

    /// Install a widget: first the system packages (administrator prompt, only on the desktop
    /// that is actually running), then - chained from `drain` - the files in the user's home.
    fn widget_install(&mut self, ctx: &egui::Context, t: Target) {
        let need_deps = self.winfo.iter().find(|i| i.target == t).map(|i| i.detected && !i.missing.is_empty() && !i.packages.is_empty()).unwrap_or(false);
        let args = |a: &str| vec!["widgets".to_string(), a.to_string(), t.id().to_string()];
        if need_deps {
            self.wpending = Some(t);
            self.run_job_ex(ctx, Job::WidgetDeps, args("deps"), None, true);
        } else {
            self.run_job_ex(ctx, Job::WidgetInstall, args("install"), None, false);
        }
    }

    fn run_job_ex(&mut self, ctx: &egui::Context, job: Job, args: Vec<String>, stdin_data: Option<String>, elevate: bool) {
        self.busy = Some(job);
        self.progress = if elevate { "Waiting for administrator authentication ...".into() } else { "Working ...".into() };
        self.add_line(Level::Info, format!("> linspot {}", args.join(" ")));
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            let exe = match std::env::current_exe() {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(Msg::Line(format!("[ERROR] cannot locate own binary: {}", e), false));
                    let _ = tx.send(Msg::Done(job, 1));
                    ctx.request_repaint();
                    return;
                }
            };
            let mut cmd = if !elevate || backend::is_root() {
                Command::new(&exe)
            } else if matches!(job, Job::Stop | Job::Kick) && backend::find("sudo").map(|s| {
                // passwordless sudo (a sudoers.d rule shipped by LinSpot) makes these silent
                let mut t = Command::new(&s);
                t.arg("-n").arg("-l").arg(&exe).args(&args);
                t.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|st| st.success()).unwrap_or(false)
            }).unwrap_or(false) {
                let mut c = Command::new("sudo");
                c.arg("-n").arg(&exe);
                c
            } else if let Some(pk) = backend::find("pkexec") {
                let mut c = Command::new(pk);
                c.arg(&exe);
                c
            } else {
                let _ = tx.send(Msg::Line("[ERROR] pkexec (polkit) not found - run install.sh or start with: sudo linspot start ...".into(), false));
                let _ = tx.send(Msg::Done(job, 127));
                ctx.request_repaint();
                return;
            };
            cmd.args(&args)
                .stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Msg::Line(format!("[ERROR] cannot launch: {}", e), false));
                    let _ = tx.send(Msg::Done(job, 1));
                    ctx.request_repaint();
                    return;
                }
            };
            if let Some(d) = stdin_data {
                if let Some(mut si) = child.stdin.take() {
                    let _ = si.write_all(d.as_bytes());
                }
            }
            let out = child.stdout.take().unwrap();
            let err = child.stderr.take().unwrap();
            let (t1, c1) = (tx.clone(), ctx.clone());
            let h1 = thread::spawn(move || {
                for l in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = t1.send(Msg::Line(l, false));
                    c1.request_repaint();
                }
            });
            let (t2, c2) = (tx.clone(), ctx.clone());
            let h2 = thread::spawn(move || {
                for l in BufReader::new(err).lines().map_while(Result::ok) {
                    let _ = t2.send(Msg::Line(l, true));
                    c2.request_repaint();
                }
            });
            let _ = h1.join();
            let _ = h2.join();
            let rc = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
            let _ = tx.send(Msg::Done(job, rc));
            ctx.request_repaint();
        });
    }

    fn validate(&self) -> Option<String> {
        if self.cfg.ssid.is_empty() {
            return Some("Network name is required".into());
        }
        if self.cfg.ssid.len() > 32 {
            return Some("Network name must be at most 32 bytes".into());
        }
        let n = self.cfg.password.len();
        if !(8..=63).contains(&n) {
            return Some("Password must be 8-63 characters".into());
        }
        None
    }

    fn start_hotspot(&mut self, ctx: &egui::Context) {
        if self.validate().is_some() {
            return;
        }
        save_cfg(&self.cfg);
        let c = backend::Config {
            ssid: self.cfg.ssid.clone(),
            password: self.cfg.password.clone(),
            iface: if self.cfg.iface.is_empty() { None } else { Some(self.cfg.iface.clone()) },
            channel: if self.cfg.channel == 0 { None } else { Some(self.cfg.channel) },
            country: None,
            band: Some(self.cfg.band.clone()),
        };
        let json = serde_json::to_string(&c).unwrap_or_default();
        let mut args = vec!["start".to_string(), "--stdin".to_string()];
        if self.cfg.debug {
            args.push("--debug".into());
        }
        self.run_job(ctx, Job::Start, args, Some(json));
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Line(raw, is_err) => {
                    let (lv, text) = parse_line(&raw, is_err);
                    if lv == Level::Step {
                        self.progress = text.clone();
                    }
                    self.add_line(lv, text);
                }
                Msg::Diag(lines) => {
                    for l in lines {
                        let (lv, t) = parse_line(&l, false);
                        self.add_line(lv, t);
                    }
                }
                Msg::Widgets(d, infos) => {
                    self.wdet = *d;
                    self.winfo = infos;
                    self.w_loading = false;
                    if !self.w_tip_shown && self.winfo.iter().any(|i| i.detected && !i.installed) {
                        self.w_tip_shown = true;
                        self.footer = ("A widget is available for your desktop - see the Widgets tab".into(), ACCENT);
                    }
                }
                Msg::Sys(s) => {
                    if self.country_sel.is_empty() {
                        self.country_sel = s.reg.clone().or_else(|| s.saved.clone()).or_else(|| s.guess.clone()).unwrap_or_default();
                    }
                    self.sys = *s;
                    self.sys_loading = false;
                }
                Msg::Done(job, rc) => {
                    self.busy = None;
                    self.progress.clear();
                    self.last_poll = long_ago();
                    let name = match job {
                        Job::Start => "Start",
                        Job::Stop => "Stop",
                        Job::Kick => "Disconnect device",
                        Job::Country => "Country change",
                        Job::WidgetDeps => "Package installation",
                        Job::WidgetInstall => "Widget installation",
                        Job::WidgetRemove => "Widget removal",
                    };
                    if rc == 0 {
                        self.footer = (format!("{} completed", name), GREEN);
                    } else if rc == 126 || rc == 127 {
                        self.add_line(Level::Error, "Administrator authentication was cancelled or denied (or pkexec is missing).".into());
                        self.footer = (format!("{} cancelled - authentication required", name), AMBER);
                    } else {
                        self.footer = (format!("{} failed (exit {}) - see Logs tab", name, rc), RED);
                        self.tab = Tab::Logs;
                        self.log_src = LogSrc::Live;
                    }
                    match job {
                        Job::Country => self.refresh_sys(ctx),
                        Job::WidgetDeps => {
                            let pending = self.wpending.take();
                            if rc == 0 {
                                if let Some(t) = pending {
                                    let a = vec!["widgets".to_string(), "install".to_string(), t.id().to_string()];
                                    self.run_job_ex(ctx, Job::WidgetInstall, a, None, false);
                                }
                            }
                        }
                        Job::WidgetInstall | Job::WidgetRemove => self.refresh_widgets(ctx),
                        _ => {}
                    }
                }
            }
        }
    }

    fn poll(&mut self) {
        if self.last_poll.elapsed() >= Duration::from_millis(1000) {
            self.last_poll = Instant::now();
            self.status = backend::status();
            self.clients = if self.status.running { backend::leases() } else { vec![] };
        }
        if self.tab == Tab::Logs && self.log_src != LogSrc::Live && self.file_poll.elapsed() >= Duration::from_millis(1000) {
            self.file_poll = Instant::now();
            self.load_file();
        }
    }

    fn load_file(&mut self) {
        self.file_lines.clear();
        if let Some(p) = self.log_src.path() {
            match std::fs::read_to_string(&p) {
                Ok(t) => {
                    let is_session = self.log_src == LogSrc::Session;
                    for l in t.lines() {
                        let (lv, tx) = if is_session { parse_line(l, false) } else { (guess_level(l), l.to_string()) };
                        self.file_lines.push(LogLine { ts: String::new(), level: lv, text: tx });
                    }
                }
                Err(_) => self.file_lines.push(LogLine {
                    ts: String::new(),
                    level: Level::Warn,
                    text: format!("{} not available (hotspot has not been started yet in this boot)", p),
                }),
            }
        }
    }

    // ---------------------------------------------------------------- panels
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(Margin::symmetric(18.0, 12.0)).stroke(Stroke::new(1.0_f32, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 12.0, ACCENT);
                    ui.painter().circle_filled(r.center(), 5.5, BG);
                    ui.label(RichText::new("LinSpot").size(22.0).strong().color(TEXT));
                    ui.label(RichText::new("Wi-Fi hotspot for any Linux").color(DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.busy.is_some() {
                            pill(ui, "WORKING", AMBER);
                        } else if self.status.running {
                            let up = uptime_str(self.status.started);
                            let txt = if up.is_empty() { format!("ON  ·  {} device(s)", self.status.clients) } else { format!("ON  ·  {} device(s)  ·  up {}", self.status.clients, up) };
                            pill(ui, &txt, GREEN);
                        } else {
                            pill(ui, "OFF", DIM);
                        }
                    });
                });
            });
    }

    fn side(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(180.0)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(Margin::same(12.0)).stroke(Stroke::new(1.0_f32, BORDER)))
            .show(ctx, |ui| {
                let errs = self.live.iter().filter(|l| l.level == Level::Error).count();
                let items = [
                    (Tab::Hotspot, format!("📶  Hotspot")),
                    (Tab::Clients, format!("👥  Clients ({})", self.clients.len())),
                    (Tab::Share, "🔗  Share / QR".to_string()),
                    (Tab::Country, "🌍  Country code".to_string()),
                    (Tab::Widgets, "🧩  Widgets".to_string()),
                    (Tab::Logs, if errs > 0 { format!("📜  Logs  [{} err]", errs) } else { "📜  Logs".to_string() }),
                    (Tab::System, "🖥  System".to_string()),
                ];
                for (t, label) in items {
                    let sel = self.tab == t;
                    let b = egui::Button::new(RichText::new(label).size(15.0).color(if sel { ACCENT } else { TEXT }))
                        .fill(if sel { Color32::from_rgba_unmultiplied(56, 189, 248, 30) } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new(ui.available_width(), 38.0));
                    if ui.add(b).clicked() {
                        self.tab = t;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(DIM).small());
                });
            });
    }

    fn footer_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("foot")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(Margin::symmetric(18.0, 6.0)).stroke(Stroke::new(1.0_f32, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy.is_some() {
                        ui.spinner();
                        ui.label(RichText::new(self.progress.clone()).color(AMBER));
                    } else {
                        dot(ui, self.footer.1);
                        ui.label(RichText::new(self.footer.0.clone()).color(DIM));
                    }
                });
            });
    }

    fn central(&mut self, ctx: &egui::Context) {
        let cur = self.tab;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(Margin::same(18.0))).show(ctx, |ui| match cur {
            Tab::Logs => self.log_tab(ui, ctx),
            tab => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match tab {
                    Tab::Hotspot => self.hotspot_tab(ui, ctx),
                    Tab::Clients => self.clients_tab(ui, ctx),
                    Tab::Share => self.share_tab(ui, ctx),
                    Tab::Country => self.country_tab(ui, ctx),
                    Tab::Widgets => self.widgets_tab(ui, ctx),
                    Tab::System => self.system_tab(ui, ctx),
                    Tab::Logs => {}
                });
            }
        });
    }

    // ------------------------------------------------------------------ tabs
    fn channel_options(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for a in &self.sys.adapters {
            if !self.cfg.iface.is_empty() && a.iface != self.cfg.iface {
                continue;
            }
            if self.cfg.band != "5" {
                v.extend(&a.ch24);
            }
            if self.cfg.band != "2.4" {
                v.extend(&a.ch5);
            }
        }
        v.sort();
        v.dedup();
        v
    }

    fn hotspot_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy.is_some();
        let running = self.status.running;

        if running {
            card(ui, "Hotspot is ON", |ui| {
                egui::Grid::new("run").num_columns(2).spacing([20.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("Network").color(DIM));
                    ui.label(RichText::new(&self.status.ssid).strong().color(GREEN));
                    ui.end_row();
                    ui.label(RichText::new("Channel").color(DIM));
                    ui.label(format!("{} ({} GHz)", self.status.channel, backend::ch_band(self.status.channel)));
                    ui.end_row();
                    ui.label(RichText::new("Mode").color(DIM));
                    ui.label(if self.status.concurrent { "Shared with your Wi-Fi (virtual AP hs0)" } else { "Dedicated adapter (no Wi-Fi client at the same time)" });
                    ui.end_row();
                    ui.label(RichText::new("Gateway").color(DIM));
                    ui.label(backend::GW);
                    ui.end_row();
                    ui.label(RichText::new("Connected devices").color(DIM));
                    ui.label(format!("{}", self.status.clients));
                    ui.end_row();
                    ui.label(RichText::new("Uptime").color(DIM));
                    ui.label(RichText::new(uptime_str(self.status.started)).color(GREEN));
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("🔗 Share / QR code").clicked() {
                        self.tab = Tab::Share;
                    }
                    if ui.button("Copy connection details").clicked() {
                        let details = format!(
                            "Wi-Fi: {}\nPassword: {}\nGateway: http://{}\nChannel: {} ({} GHz)",
                            self.status.ssid,
                            self.cfg.password,
                            backend::GW,
                            self.status.channel,
                            backend::ch_band(self.status.channel)
                        );
                        ui.ctx().output_mut(|o| o.copied_text = details);
                        self.footer = ("Connection details copied".into(), GREEN);
                    }
                    if ui.button("Open gateway page").clicked() {
                        let _ = Command::new("xdg-open").arg(format!("http://{}", backend::GW)).spawn();
                        self.footer = ("Opening the hotspot admin page".into(), ACCENT);
                    }
                });
            });
        }

        card(ui, "Hotspot settings", |ui| {
            ui.add_enabled_ui(!running && !busy, |ui| {
                egui::Grid::new("form").num_columns(2).spacing([16.0, 12.0]).min_col_width(130.0).show(ui, |ui| {
                    ui.label("Network name (SSID)");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.ssid).hint_text("MyHotspot").desired_width(f32::INFINITY));
                    ui.end_row();

                    ui.label("Password");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.password).password(!self.show_pw).hint_text("8-63 characters").desired_width(260.0));
                        ui.checkbox(&mut self.show_pw, "Show");
                        if ui.button("Generate").clicked() {
                            self.cfg.password = random_password();
                            self.show_pw = true;
                        }
                    });
                    ui.end_row();

                    ui.label("Band");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("band")
                            .selected_text(match self.cfg.band.as_str() {
                                "2.4" => "2.4 GHz",
                                "5" => "5 GHz",
                                _ => "Auto (recommended)",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.cfg.band, "auto".to_string(), "Auto (recommended)");
                                ui.selectable_value(&mut self.cfg.band, "2.4".to_string(), "2.4 GHz");
                                ui.selectable_value(&mut self.cfg.band, "5".to_string(), "5 GHz");
                            });
                    });
                    ui.end_row();

                    ui.label("Channel");
                    let opts = self.channel_options();
                    egui::ComboBox::from_id_salt("chan")
                        .selected_text(if self.cfg.channel == 0 { "Auto".to_string() } else { self.cfg.channel.to_string() })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cfg.channel, 0u32, "Auto");
                            for c in opts {
                                ui.selectable_value(&mut self.cfg.channel, c, c.to_string());
                            }
                        });
                    ui.end_row();

                    ui.label("Wi-Fi adapter");
                    let ifaces: Vec<String> = self.sys.adapters.iter().map(|a| a.iface.clone()).collect();
                    egui::ComboBox::from_id_salt("iface")
                        .selected_text(if self.cfg.iface.is_empty() { "Auto".to_string() } else { self.cfg.iface.clone() })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cfg.iface, String::new(), "Auto");
                            for i in ifaces {
                                ui.selectable_value(&mut self.cfg.iface, i.clone(), i);
                            }
                        });
                    ui.end_row();

                    ui.label("Detailed debug log");
                    ui.checkbox(&mut self.cfg.debug, "Log every command and its output");
                    ui.end_row();
                });
            });
            if let Some(msg) = self.validate() {
                if !self.cfg.password.is_empty() || !self.cfg.ssid.is_empty() {
                    ui.label(RichText::new(msg).color(AMBER));
                }
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Regulatory country: {}", self.sys.reg.clone().unwrap_or_else(|| "not set (world domain)".into()))).color(DIM));
                if ui.link("change").clicked() {
                    self.tab = Tab::Country;
                }
            });
            if self.cfg.band == "5" || self.cfg.channel > 14 {
                ui.label(RichText::new("5 GHz needs a country code set and a card/country that allows AP mode there.").color(DIM).small());
            }
        });

        let (label, color) = if running { ("Turn hotspot OFF", RED) } else { ("Turn hotspot ON", GREEN) };
        let can = !busy && (running || self.validate().is_none());
        let btn = egui::Button::new(RichText::new(label).size(18.0).strong().color(Color32::BLACK))
            .fill(color)
            .rounding(Rounding::same(12.0))
            .min_size(Vec2::new(ui.available_width(), 52.0));
        if ui.add_enabled(can, btn).clicked() {
            if running {
                self.run_job(ctx, Job::Stop, vec!["stop".into()], None);
            } else {
                self.start_hotspot(ctx);
            }
        }
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(self.progress.clone()).color(AMBER));
            });
        }
        ui.add_space(8.0);
        ui.label(RichText::new("The hotspot keeps running if you close this window. Your Wi-Fi is disconnected, the hotspot starts, then the same Wi-Fi is reconnected.").color(DIM).small());
    }

    fn clients_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy.is_some();
        card(ui, "Connected devices", |ui| {
            if !self.status.running {
                ui.label(RichText::new("Hotspot is OFF.").color(DIM));
            } else if self.clients.is_empty() {
                ui.label(RichText::new("No device has received an address yet.").color(DIM));
            } else {
                let rows: Vec<(String, String, String, u64)> = self
                    .clients
                    .iter()
                    .map(|c| (c.ip.clone(), c.mac.clone(), c.host.clone(), c.expires))
                    .collect();
                egui::Grid::new("cl").striped(true).num_columns(5).spacing([24.0, 8.0]).show(ui, |ui| {
                    for h in ["IP address", "MAC address", "Hostname", "Lease expires", ""] {
                        ui.label(RichText::new(h).strong().color(ACCENT));
                    }
                    ui.end_row();
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    for (ip, mac, host, expires) in rows {
                        ui.label(RichText::new(&ip).monospace());
                        ui.label(RichText::new(&mac).monospace());
                        ui.label(&host);
                        let left = expires.saturating_sub(now);
                        ui.label(if expires == 0 { "infinite".to_string() } else { format!("in {}h {:02}m", left / 3600, (left % 3600) / 60) });
                        if ui.add_enabled(!busy, egui::Button::new(RichText::new("Disconnect").color(RED))).clicked() {
                            self.run_job(ctx, Job::Kick, vec!["kick".into(), mac.clone()], None);
                        }
                        ui.end_row();
                    }
                });
            }
        });
        ui.label(RichText::new("List is based on DHCP leases; a device that just left may stay listed until its lease ends. 'Disconnect' deauthenticates the device - it can rejoin unless it is blocked at its end.").color(DIM).small());
    }

    fn share_tab(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let key = (self.cfg.ssid.clone(), self.cfg.password.clone());
        if self.qr_key != key {
            self.qr_key = key;
            self.qr = if self.cfg.ssid.is_empty() || self.cfg.password.is_empty() {
                None
            } else {
                Some(qr::wifi_qr(&self.cfg.ssid, &self.cfg.password))
            };
        }
        card(ui, "Connect by QR code", |ui| {
            ui.label(RichText::new("Point a phone camera at the code: Android, iOS and Windows offer to join the network automatically (no typing the password). The code uses the network name and password from the Hotspot tab.").color(DIM));
            ui.add_space(10.0);
            match self.qr.clone() {
                None => {
                    ui.label(RichText::new("Enter a network name and a password on the Hotspot tab first.").color(AMBER));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(format!("Could not build the QR code: {}", e)).color(RED));
                }
                Some(Ok(q)) => {
                    let side = (ui.available_width() - 24.0).min(300.0).max(140.0);
                    let (resp, painter) = ui.allocate_painter(Vec2::splat(side), egui::Sense::hover());
                    let cell = side / (q.size + 8) as f32;
                    painter.rect(resp.rect, Rounding::same(10.0), Color32::WHITE, Stroke::new(1.0_f32, BORDER));
                    let quiet = 4.0 * cell;
                    for y in 0..q.size {
                        for x in 0..q.size {
                            if q.is_dark(x, y) {
                                let p0 = resp.rect.min + egui::vec2(quiet + x as f32 * cell, quiet + y as f32 * cell);
                                painter.rect(egui::Rect::from_min_size(p0, Vec2::splat(cell + 0.5)), Rounding::same(0.0), Color32::BLACK, Stroke::NONE);
                            }
                        }
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let qr = self.qr.clone();
                        if ui.add_enabled(qr.is_some(), egui::Button::new("Save QR as PNG")).clicked() {
                            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                            let dir = if std::path::Path::new(&format!("{}/Pictures", home)).is_dir() { format!("{}/Pictures", home) } else { home };
                            let p = format!("{}/linspot-wifi-qr.png", dir);
                            match qr {
                                Some(Ok(q)) => match qr::save_png(&q, 8, &p) {
                                    Ok(()) => self.footer = (format!("QR code saved to {}", p), GREEN),
                                    Err(e) => self.footer = (format!("Could not save the QR code: {}", e), RED),
                                },
                                _ => {}
                            }
                        }
                        if ui.button("Copy Wi-Fi connect string").clicked() {
                            ui.ctx().output_mut(|o| o.copied_text = qr::wifi_string(&self.cfg.ssid, &self.cfg.password));
                            self.footer = ("Connect string copied - paste it into a chat or a QR generator".into(), GREEN);
                        }
                    });
                }
            }
        });
        let running = self.status.running;
        card(ui, "Other ways to share", |ui| {
            if running {
                ui.horizontal(|ui| {
                    if ui.button("Copy connection details").clicked() {
                        let details = format!(
                            "Wi-Fi: {}\nPassword: {}\nGateway: http://{}\nChannel: {} ({} GHz)",
                            self.status.ssid,
                            self.cfg.password,
                            backend::GW,
                            self.status.channel,
                            backend::ch_band(self.status.channel)
                        );
                        ui.ctx().output_mut(|o| o.copied_text = details);
                        self.footer = ("Connection details copied".into(), GREEN);
                    }
                    if ui.button("Open gateway page").clicked() {
                        let _ = Command::new("xdg-open").arg(format!("http://{}", backend::GW)).spawn();
                        self.footer = ("Opening the hotspot admin page".into(), ACCENT);
                    }
                });
            } else {
                ui.label(RichText::new("Start the hotspot to copy live connection details or open the gateway page.").color(DIM));
            }
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Desktop widgets: the Plasma popup has a 'Show QR' button, and  linspot qr --png /tmp/linspot-qr.png  writes the code from any terminal or script.")).color(DIM).small());
        });
    }

    fn country_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy.is_some();
        card(ui, "Wi-Fi regulatory country", |ui| {
            ui.horizontal(|ui| {
                ui.label("Current:");
                match &self.sys.reg {
                    Some(c) => ui.label(RichText::new(c).strong().color(GREEN)),
                    None => ui.label(RichText::new("00 (world domain - 5 GHz hotspot usually blocked)").color(AMBER)),
                };
            });
            if let Some(g) = &self.sys.guess {
                ui.label(RichText::new(format!("Suggested from your timezone: {}", g)).color(DIM));
            }
            if let Some(s) = &self.sys.saved {
                ui.label(RichText::new(format!("Saved by LinSpot: {}", s)).color(DIM));
            }
            ui.label(RichText::new("Pick the country you are physically in; channels and transmit power must follow local law.").color(DIM).small());
        });
        card(ui, "Choose country", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.country_filter).hint_text("Search name or code ...").desired_width(240.0));
                ui.label(RichText::new(format!("Selected: {}", if self.country_sel.is_empty() { "-" } else { self.country_sel.as_str() })).strong().color(ACCENT));
            });
            let f = self.country_filter.to_lowercase();
            egui::Frame::none().fill(Color32::from_rgb(10, 12, 17)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(6.0)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (code, name) in COUNTRIES {
                        if !f.is_empty() && !code.to_lowercase().contains(&f) && !name.to_lowercase().contains(&f) {
                            continue;
                        }
                        let sel = self.country_sel == *code;
                        if ui.selectable_label(sel, format!("{}   {}", code, name)).clicked() {
                            self.country_sel = code.to_string();
                        }
                    }
                });
            });
            ui.checkbox(&mut self.country_persist, "Make permanent (survives reboot)");
            ui.horizontal(|ui| {
                let ok = !busy && self.country_sel.len() == 2;
                if ui.add_enabled(ok, egui::Button::new(RichText::new("Apply country").strong().color(Color32::BLACK)).fill(ACCENT)).clicked() {
                    let mut args = vec!["country".to_string(), self.country_sel.clone()];
                    if self.country_persist {
                        args.push("--persist".into());
                    }
                    self.run_job(ctx, Job::Country, args, None);
                }
                if ui.button("Use suggested").clicked() {
                    if let Some(g) = self.sys.guess.clone() {
                        self.country_sel = g;
                    }
                }
                if ui.button("Refresh").clicked() {
                    self.refresh_sys(ctx);
                }
            });
        });
    }

    fn widgets_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.busy.is_some();
        let det = self.wdet.clone();
        let infos = self.winfo.clone();
        let pm = widgets::pkg_manager();
        let mut act: Option<(Target, bool)> = None; // (widget, remove?)
        let mut rescan = false;

        card(ui, "Desktop widgets", |ui| {
            ui.horizontal(|ui| {
                let d = if det.xdg.is_empty() { "unknown" } else { det.xdg.as_str() };
                let s = if det.session.is_empty() { "unknown" } else { det.session.as_str() };
                ui.label(RichText::new(format!("Desktop: {}   Session: {}", d, s)).color(TEXT));
                ui.label(RichText::new(format!("   Distro: {}   Package manager: {}", self.sys.distro, pm)).color(DIM));
                if ui.button("Re-scan").clicked() {
                    rescan = true;
                }
                if self.w_loading {
                    ui.spinner();
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "The widgets are built into LinSpot. Install the one for your desktop: missing system packages are installed for your distro (administrator prompt) and the widget files go into your home folder. Then follow the 'How to enable it' steps.",
                )
                .color(DIM),
            );
        });

        if infos.is_empty() {
            ui.label(RichText::new("Scanning ...").color(DIM));
        }
        for i in &infos {
            let t = i.target;
            card(ui, t.title(), |ui| {
                ui.horizontal(|ui| {
                    if i.detected {
                        pill(ui, "YOUR DESKTOP", GREEN);
                    }
                    if i.installed {
                        pill(ui, "INSTALLED", ACCENT);
                    } else {
                        pill(ui, "NOT INSTALLED", DIM);
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(t.summary()).color(DIM));
                if !i.note.is_empty() {
                    ui.label(RichText::new(i.note.clone()).color(AMBER));
                }
                if !i.missing.is_empty() {
                    ui.label(RichText::new(format!("Missing on this system: {}", i.missing.join(", "))).color(AMBER));
                    if t == Target::Dms {
                        ui.label(RichText::new("DankMaterialShell has to be installed first. Run this in a terminal (as your normal user):").color(DIM));
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(widgets::DMS_INSTALL_CMD).monospace().color(TEXT)).selectable(true));
                            if ui.button("Copy").clicked() {
                                ui.ctx().output_mut(|o| o.copied_text = widgets::DMS_INSTALL_CMD.to_string());
                            }
                        });
                        ui.label(RichText::new("The widget files can already be installed now; they work once DMS is running (docs: https://danklinux.com).").color(DIM).small());
                    } else if i.detected && !i.packages.is_empty() {
                        ui.label(RichText::new(format!("Will install with {}: {}", pm, i.packages.join(" "))).color(DIM).small());
                    } else if !i.detected {
                        ui.label(RichText::new("This is not your running desktop, so no system packages are installed - only the widget files.").color(DIM).small());
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if i.installed { "Reinstall / update" } else if i.detected && !i.missing.is_empty() && !i.packages.is_empty() { "Install packages + widget" } else { "Install widget" };
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new(label).strong())).clicked() {
                        act = Some((t, false));
                    }
                    if i.installed && ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                        act = Some((t, true));
                    }
                });
                ui.add_space(4.0);
                ui.push_id(t.id(), |ui| {
                    egui::CollapsingHeader::new(RichText::new("How to enable it").color(ACCENT)).default_open(i.installed).show(ui, |ui| {
                        for (n, step) in widgets::enable_steps(t).iter().enumerate() {
                            ui.label(RichText::new(format!("{}. {}", n + 1, step)).color(TEXT));
                        }
                    });
                });
            });
        }

        if rescan {
            self.refresh_widgets(ctx);
        }
        if let Some((t, remove)) = act {
            if remove {
                let a = vec!["widgets".to_string(), "remove".to_string(), t.id().to_string()];
                self.run_job_ex(ctx, Job::WidgetRemove, a, None, false);
            } else {
                self.widget_install(ctx, t);
            }
        }
    }

    fn system_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, "System", |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Distribution: {}", self.sys.distro)).color(TEXT));
                ui.label(RichText::new(format!("   Package manager: {}", self.sys.pm)).color(DIM));
                if ui.button("Re-scan").clicked() {
                    self.refresh_sys(ctx);
                }
                if self.sys_loading {
                    ui.spinner();
                }
            });
        });
        card(ui, "Dependencies", |ui| {
            egui::Grid::new("deps").striped(true).num_columns(3).spacing([24.0, 8.0]).show(ui, |ui| {
                for t in &self.sys.tools {
                    ui.label(RichText::new(t.name).monospace());
                    if t.found {
                        ui.label(RichText::new("found").color(GREEN));
                    } else if t.required {
                        ui.label(RichText::new("MISSING").strong().color(RED));
                    } else {
                        ui.label(RichText::new("not found (optional)").color(AMBER));
                    }
                    ui.label(RichText::new(t.purpose).color(DIM));
                    ui.end_row();
                }
            });
            if self.sys.tools.iter().any(|t| t.required && !t.found) {
                ui.add_space(6.0);
                ui.label(RichText::new("Install the missing tools with:  sudo ./install.sh --deps-only").color(AMBER));
            }
        });
        card(ui, "Wi-Fi adapters", |ui| {
            if self.sys.adapters.is_empty() {
                ui.label(RichText::new("No Wi-Fi adapter detected (or 'iw' is missing / radio is blocked).").color(AMBER));
            }
            for a in &self.sys.adapters {
                adapter_view(ui, a);
            }
        });
    }

    fn log_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal_wrapped(|ui| {
            let before = self.log_src;
            egui::ComboBox::from_id_salt("logsrc").selected_text(self.log_src.label()).show_ui(ui, |ui| {
                for s in LogSrc::ALL {
                    ui.selectable_value(&mut self.log_src, s, s.label());
                }
            });
            if before != self.log_src {
                self.load_file();
            }
            ui.checkbox(&mut self.f_debug, "Debug");
            ui.checkbox(&mut self.f_info, "Info");
            ui.checkbox(&mut self.f_warn, "Warn");
            ui.checkbox(&mut self.f_error, "Error");
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search ...").desired_width(190.0));
        });

        let flags = (self.f_debug, self.f_info, self.f_warn, self.f_error);
        let q = self.search.to_lowercase();
        let show_ts = self.show_ts;
        let autoscroll = self.autoscroll;
        let src_lines: &Vec<LogLine> = if self.log_src == LogSrc::Live { &self.live } else { &self.file_lines };
        let total = src_lines.len();
        let vis: Vec<&LogLine> = src_lines.iter().filter(|l| lvl_ok(l.level, flags) && (q.is_empty() || l.text.to_lowercase().contains(&q))).collect();
        let fmt = |l: &LogLine| {
            if l.ts.is_empty() {
                format!("[{}] {}", level_tag(l.level).trim(), l.text)
            } else {
                format!("{} [{}] {}", l.ts, level_tag(l.level).trim(), l.text)
            }
        };

        let mut act: Option<Act> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{} / {} lines", vis.len(), total)).color(DIM));
            if ui.button("Copy").clicked() {
                act = Some(Act::Copy(vis.iter().map(|l| fmt(*l)).collect::<Vec<_>>().join("\n")));
            }
            if ui.button("Save to file").clicked() {
                act = Some(Act::Save(vis.iter().map(|l| fmt(*l)).collect::<Vec<_>>().join("\n")));
            }
            if ui.button("Clear").clicked() {
                act = Some(Act::Clear);
            }
            if ui.button("Run diagnostics").clicked() {
                act = Some(Act::Diag);
            }
        });
        let mut ts_flag = show_ts;
        let mut as_flag = autoscroll;
        ui.horizontal(|ui| {
            ui.checkbox(&mut ts_flag, "Timestamps");
            ui.checkbox(&mut as_flag, "Auto-scroll");
        });

        let h = (ui.available_height() - 4.0).max(120.0);
        egui::Frame::none().fill(Color32::from_rgb(9, 11, 15)).stroke(Stroke::new(1.0_f32, BORDER)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(10.0)).show(ui, |ui| {
            ui.set_min_size(Vec2::new(ui.available_width(), h - 22.0));
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(h - 22.0).stick_to_bottom(as_flag).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 2.0);
                let start = vis.len().saturating_sub(2500);
                if start > 0 {
                    ui.label(RichText::new(format!("... {} older lines hidden (use Copy / Save for everything)", start)).color(DIM).monospace());
                }
                for l in &vis[start..] {
                    ui.horizontal_wrapped(|ui| {
                        if ts_flag && !l.ts.is_empty() {
                            ui.label(RichText::new(&l.ts).monospace().color(DIM));
                        }
                        ui.label(RichText::new(level_tag(l.level)).monospace().color(level_color(l.level)).strong());
                        ui.label(RichText::new(&l.text).monospace().color(level_color(l.level)));
                    });
                }
            });
        });

        self.show_ts = ts_flag;
        self.autoscroll = as_flag;
        match act {
            Some(Act::Copy(t)) => {
                ctx.output_mut(|o| o.copied_text = t);
                self.footer = ("Log copied to clipboard".into(), GREEN);
            }
            Some(Act::Save(t)) => {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
                let p = format!("{}/linspot-{}.log", home, now_hms().replace(':', ""));
                match std::fs::write(&p, t) {
                    Ok(_) => self.footer = (format!("Saved log to {}", p), GREEN),
                    Err(e) => self.footer = (format!("Could not save log: {}", e), RED),
                }
            }
            Some(Act::Clear) => {
                if self.log_src == LogSrc::Live {
                    self.live.clear();
                } else {
                    self.footer = ("Backend log files are cleared on the next hotspot start".into(), DIM);
                }
            }
            Some(Act::Diag) => {
                self.log_src = LogSrc::Live;
                self.run_diag(ctx);
            }
            None => {}
        }
    }
}

fn adapter_view(ui: &mut egui::Ui, a: &Adapter) {
    egui::Frame::none().fill(Color32::from_rgb(22, 27, 36)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(12.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&a.iface).strong().size(16.0).color(ACCENT));
            ui.label(RichText::new(format!("({})", a.phy)).color(DIM));
            match &a.ssid {
                Some(s) => ui.label(RichText::new(format!("connected to '{}' on channel {}", s, a.channel.map(|c| c.to_string()).unwrap_or_else(|| "?".into()))).color(GREEN)),
                None => ui.label(RichText::new("not connected").color(DIM)),
            };
        });
        if a.concurrent {
            ui.label(RichText::new(if a.single { "Share + stay online: YES (same channel as your Wi-Fi)" } else { "Share + stay online: YES (independent channels)" }).color(GREEN));
        } else {
            ui.label(RichText::new("Share + stay online: NO - use Ethernet / USB tether as the internet source").color(AMBER));
        }
        let f = |v: &Vec<u32>| if v.is_empty() { "none".to_string() } else { v.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" ") };
        ui.label(RichText::new(format!("AP channels  2.4 GHz: {}", f(&a.ch24))).monospace().color(DIM));
        ui.label(RichText::new(format!("AP channels  5 GHz:   {}", f(&a.ch5))).monospace().color(DIM));
    });
    ui.add_space(6.0);
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);
        self.poll();
        self.top_bar(ctx);
        self.footer_bar(ctx);
        self.side(ctx);
        self.central(ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}
