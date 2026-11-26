use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG: OnceLock<Mutex<Option<fs::File>>> = OnceLock::new();

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    base.join("melissm")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn init() {
    let dir = state_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("melissm.log");
    if fs::metadata(&path).map(|m| m.len() > 1024 * 1024).unwrap_or(false) {
        let _ = fs::rename(&path, dir.join("melissm.log.old"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let _ = LOG.set(Mutex::new(file));
    std::panic::set_hook(Box::new(|info| {
        line(&format!("panic: {info}"));
    }));
    line(&format!("start pid={} version={}", std::process::id(), env!("CARGO_PKG_VERSION")));
}

pub fn line(msg: &str) {
    let Some(lock) = LOG.get() else {
        return;
    };
    let Ok(mut guard) = lock.try_lock() else {
        return;
    };
    let Some(file) = guard.as_mut() else {
        return;
    };
    let _ = writeln!(file, "[{}] {msg}", now());
    let _ = file.flush();
}
