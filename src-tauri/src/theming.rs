use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    process::Command,
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

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn state_dir() -> String {
    env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| format!("{}/.local/state", env::var("HOME").unwrap_or_default()))
}

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

fn system_dark() -> bool {
    run("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"])
        .map(|o| o.contains("dark"))
        .unwrap_or(true)
}

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

pub fn current(opts: &ThemeOpts) -> Option<BTreeMap<String, String>> {
    let mode = match opts.mode {
        Mode::Dark => "dark",
        Mode::Light => "light",
        Mode::Auto => {
            if system_dark() { "dark" } else { "light" }
        }
    };
    if matches!(opts.source, Source::Color(_)) {
        from_matugen(opts, mode)
    } else {
        from_matugen(opts, mode).or_else(from_caelestia)
    }
}

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
