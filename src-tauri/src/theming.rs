use serde::{Deserialize, Serialize};
#[cfg(not(windows))]
use serde_json::Value;
#[cfg(not(windows))]
use std::{collections::BTreeMap, env, fs, process::Command};
#[cfg(windows)]
use std::collections::BTreeMap;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Wallpaper,
    Color(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ThemeOpts {
    pub mode: Mode,
    pub scheme: String,
    pub source: Source,
}

impl Default for ThemeOpts {
    fn default() -> Self {
        Self { mode: Mode::Dark, scheme: "tonal-spot".into(), source: Source::Wallpaper }
    }
}

pub type SharedOpts = Arc<Mutex<ThemeOpts>>;

#[cfg(not(windows))]
fn kebab(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_uppercase() {
            out.push('-');
            out.extend(c.to_lowercase());
        } else if c == '_' {
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out
}

fn hex(s: &str) -> String {
    if s.starts_with('#') { s.to_string() } else { format!("#{s}") }
}

fn resolved_mode(opts: &ThemeOpts) -> &'static str {
    match opts.mode {
        Mode::Dark => "dark",
        Mode::Light => "light",
        Mode::Auto => {
            if system_dark() { "dark" } else { "light" }
        }
    }
}

#[cfg(windows)]
fn reg_read(path: &str, key: &str) -> Option<u32> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let sub = hkcu.open_subkey(path).ok()?;
    sub.get_value::<u32, _>(key).ok()
}

#[cfg(windows)]
fn accent() -> Option<String> {
    let dword = reg_read(r"Software\Microsoft\Windows\CurrentVersion\Explorer", "AccentColor")
        .or_else(|| reg_read(r"Software\Microsoft\Windows\DWM", "ColorizationColor"))?;
    let r = dword & 0xff;
    let g = (dword >> 8) & 0xff;
    let b = (dword >> 16) & 0xff;
    Some(format!("#{:02x}{:02x}{:02x}", r, g, b))
}

#[cfg(windows)]
fn system_dark() -> bool {
    reg_read(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme")
        .map(|v| v == 0)
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
fn state_dir() -> String {
    env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| format!("{}/.local/state", env::var("HOME").unwrap_or_default()))
}

#[cfg(not(windows))]
pub fn wallpaper() -> Option<String> {
    let exists = |p: &str| (!p.is_empty() && fs::metadata(p).is_ok()).then(|| p.to_string());
    if let Ok(p) = fs::read_to_string(format!("{}/caelestia/wallpaper/path.txt", state_dir())) {
        if let Some(p) = exists(p.trim()) {
            return Some(p);
        }
    }
    if let Some(o) = run("swww", &["query"]) {
        if let Some(p) = o.lines().find_map(|l| l.split("image: ").nth(1)).and_then(|p| exists(p.trim())) {
            return Some(p);
        }
    }
    if let Some(o) = run("hyprctl", &["hyprpaper", "listactive"]) {
        if let Some(p) = o.lines().find_map(|l| l.split(" = ").nth(1)).and_then(|p| exists(p.trim())) {
            return Some(p);
        }
    }
    for key in ["picture-uri-dark", "picture-uri"] {
        if let Some(o) = run("gsettings", &["get", "org.gnome.desktop.background", key]) {
            let p = o.trim().trim_matches('\'').trim_start_matches("file://").to_string();
            if let Some(p) = exists(&p) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn system_dark() -> bool {
    run("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])
        .map(|o| o.contains("dark"))
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn parse_matugen_json(out: &str, mode: &str) -> Option<BTreeMap<String, String>> {
    let json: Value = serde_json::from_str(out).ok()?;
    let colors = json.get("colors")?.as_object()?;
    let mut map = BTreeMap::new();
    for (name, v) in colors {
        if let Some(c) = v.get(mode).and_then(Value::as_str) {
            map.insert(kebab(name), c.to_string());
        } else if let Some(c) = v.get(mode).and_then(|m| m.get("color")).and_then(Value::as_str) {
            map.insert(kebab(name), c.to_string());
        } else if let Some(c) = v.get("dark").and_then(Value::as_str) {
            map.insert(kebab(name), c.to_string());
        }
    }
    (!map.is_empty()).then_some(map)
}

#[cfg(not(windows))]
fn from_matugen(opts: &ThemeOpts, mode: &str) -> Option<BTreeMap<String, String>> {
    let scheme = format!("scheme-{}", opts.scheme);
    let mut out = match &opts.source {
        Source::Wallpaper => {
            let path = wallpaper()?;
            run("matugen", &[
                "--json", "hex", "--dry-run", "--prefer", "saturation",
                "-m", mode, "-t", &scheme, "image", &path,
            ])?
        }
        Source::Color(c) => {
            let c = hex(c);
            run("matugen", &[
                "--json", "hex", "--dry-run",
                "-m", mode, "-t", &scheme, "color", "hex", &c,
            ])?
        }
    };
    out = out.replace("\\u003d", "=");
    parse_matugen_json(&out, mode)
}

#[cfg(not(windows))]
fn from_caelestia() -> Option<BTreeMap<String, String>> {
    let raw = fs::read_to_string(format!("{}/caelestia/scheme.json", state_dir())).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;
    let map: BTreeMap<String, String> = json
        .get("colours")?
        .as_object()?
        .iter()
        .filter_map(|(k, v)| Some((kebab(k), hex(v.as_str()?))))
        .collect();
    (!map.is_empty()).then_some(map)
}

#[cfg(windows)]
fn base_color(opts: &ThemeOpts) -> Option<String> {
    match &opts.source {
        Source::Color(c) => Some(hex(c)),
        Source::Wallpaper => accent(),
    }
}

#[cfg(windows)]
fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

#[cfg(windows)]
fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to8 = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    format!("#{:02x}{:02x}{:02x}", to8(r1), to8(g1), to8(b1))
}

#[cfg(windows)]
fn parse_hex(h: &str) -> (f64, f64, f64) {
    let h = h.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0x808080);
    (
        ((v >> 16) & 0xff) as f64 / 255.0,
        ((v >> 8) & 0xff) as f64 / 255.0,
        (v & 0xff) as f64 / 255.0,
    )
}

#[cfg(windows)]
fn tone(base: &str, l: f64, sat_mul: f64, hue_shift: f64) -> String {
    let (r, g, b) = parse_hex(base);
    let (mut h, mut s, _) = rgb_to_hsl(r, g, b);
    h = (h + hue_shift).rem_euclid(360.0);
    s = (s * sat_mul).clamp(0.0, 1.0);
    hsl_to_hex(h, s, l)
}

#[cfg(windows)]
fn palette(base: &str, mode: &str) -> BTreeMap<String, String> {
    let dark = mode == "dark";
    let (p, pc, pco, op, s, sc, sco, os, osv, o, ov, t, tc, tco, ot) = if dark {
        (
            tone(base, 0.80, 1.0, 0.0),
            tone(base, 0.30, 1.0, 0.0),
            tone(base, 0.90, 1.0, 0.0),
            tone(base, 0.20, 1.0, 0.0),
            tone(base, 0.80, 0.45, 0.0),
            tone(base, 0.30, 0.45, 0.0),
            tone(base, 0.90, 0.45, 0.0),
            tone(base, 0.20, 0.35, 0.0),
            tone(base, 0.90, 0.35, 0.0),
            tone(base, 0.60, 0.35, 0.0),
            tone(base, 0.30, 0.30, 0.0),
            tone(base, 0.80, 0.85, 60.0),
            tone(base, 0.30, 0.85, 60.0),
            tone(base, 0.90, 0.85, 60.0),
            tone(base, 0.20, 0.85, 60.0),
        )
    } else {
        (
            tone(base, 0.40, 1.0, 0.0),
            tone(base, 0.90, 1.0, 0.0),
            tone(base, 0.10, 1.0, 0.0),
            tone(base, 1.0, 1.0, 0.0),
            tone(base, 0.40, 0.45, 0.0),
            tone(base, 0.90, 0.45, 0.0),
            tone(base, 0.10, 0.45, 0.0),
            tone(base, 0.10, 0.35, 0.0),
            tone(base, 0.30, 0.35, 0.0),
            tone(base, 0.50, 0.35, 0.0),
            tone(base, 0.80, 0.30, 0.0),
            tone(base, 0.40, 0.85, 60.0),
            tone(base, 0.90, 0.85, 60.0),
            tone(base, 0.10, 0.85, 60.0),
            tone(base, 1.0, 0.85, 60.0),
        )
    };
    let (surf, low, cont, high, hi, lo, lowest, dim, bright, isurf, isurfo, iprimary) = if dark {
        (
            tone(base, 0.06, 0.25, 0.0),
            tone(base, 0.10, 0.25, 0.0),
            tone(base, 0.12, 0.25, 0.0),
            tone(base, 0.17, 0.25, 0.0),
            tone(base, 0.22, 0.25, 0.0),
            tone(base, 0.04, 0.25, 0.0),
            tone(base, 0.00, 0.25, 0.0),
            tone(base, 0.06, 0.25, 0.0),
            tone(base, 0.24, 0.25, 0.0),
            tone(base, 0.90, 0.25, 0.0),
            tone(base, 0.20, 0.25, 0.0),
            tone(base, 0.40, 1.0, 0.0),
        )
    } else {
        (
            tone(base, 0.98, 0.25, 0.0),
            tone(base, 0.96, 0.25, 0.0),
            tone(base, 0.94, 0.25, 0.0),
            tone(base, 0.92, 0.25, 0.0),
            tone(base, 0.90, 0.25, 0.0),
            tone(base, 1.00, 0.25, 0.0),
            tone(base, 1.00, 0.25, 0.0),
            tone(base, 0.87, 0.25, 0.0),
            tone(base, 0.98, 0.25, 0.0),
            tone(base, 0.20, 0.25, 0.0),
            tone(base, 0.95, 0.25, 0.0),
            tone(base, 0.80, 1.0, 0.0),
        )
    };
    let (err, erro, errc, errco) = if dark {
        ("#f2b8b5", "#601410", "#8c1d18", "#f9dedc")
    } else {
        ("#b3261e", "#ffffff", "#f9dedc", "#410e0b")
    };
    BTreeMap::from([
        ("primary".into(), p),
        ("on-primary".into(), op),
        ("primary-container".into(), pc),
        ("on-primary-container".into(), pco),
        ("secondary".into(), s),
        ("on-secondary".into(), os),
        ("secondary-container".into(), sc),
        ("on-secondary-container".into(), sco),
        ("tertiary".into(), t),
        ("on-tertiary".into(), ot),
        ("tertiary-container".into(), tc),
        ("on-tertiary-container".into(), tco),
        ("error".into(), err.into()),
        ("on-error".into(), erro.into()),
        ("error-container".into(), errc.into()),
        ("on-error-container".into(), errco.into()),
        ("surface".into(), surf),
        ("surface-dim".into(), dim),
        ("surface-bright".into(), bright),
        ("surface-container-lowest".into(), lowest),
        ("surface-container-low".into(), low),
        ("surface-container".into(), cont),
        ("surface-container-high".into(), high),
        ("surface-container-highest".into(), hi),
        ("on-surface".into(), osv.clone()),
        ("on-surface-variant".into(), osv),
        ("outline".into(), o),
        ("outline-variant".into(), ov),
        ("inverse-surface".into(), isurf),
        ("inverse-on-surface".into(), isurfo),
        ("inverse-primary".into(), iprimary),
        ("surface-tint".into(), p.clone()),
        ("scrim".into(), "#000000".into()),
        ("shadow".into(), "#000000".into()),
    ])
}

#[cfg(windows)]
pub fn current(opts: &ThemeOpts) -> Option<BTreeMap<String, String>> {
    let mode = resolved_mode(opts);
    let base = base_color(opts)?;
    Some(palette(&base, mode))
}

#[cfg(not(windows))]
pub fn current(opts: &ThemeOpts) -> Option<BTreeMap<String, String>> {
    let mode = resolved_mode(opts);
    if matches!(opts.source, Source::Color(_)) {
        from_matugen(opts, mode)
    } else {
        from_matugen(opts, mode).or_else(from_caelestia)
    }
}

#[cfg(windows)]
pub fn watch(app: AppHandle, opts: SharedOpts) {
    thread::spawn(move || {
        let mut last: Option<String> = None;
        loop {
            let o = opts.lock().unwrap().clone();
            let mode = resolved_mode(&o);
            let sig = base_color(&o).map(|c| format!("{c} {mode}"));
            if sig != last && sig.is_some() {
                last = sig;
                if let Some(p) = current(&o) {
                    let _ = app.emit("theme-changed", p);
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(not(windows))]
pub fn watch(app: AppHandle, opts: SharedOpts) {
    thread::spawn(move || {
        let mut last: Option<String> = None;
        loop {
            let wp = wallpaper();
            if wp != last {
                last = wp;
                let opts = opts.lock().unwrap().clone();
                if let Some(palette) = current(&opts) {
                    let _ = app.emit("theme-changed", palette);
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}
