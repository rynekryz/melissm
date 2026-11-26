use crate::crypto;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_AUDIO_CACHE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct Cache {
    root: PathBuf,
    shm: bool,
}

fn prune_tmp(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        if e.path().extension().is_some_and(|x| x == "tmp") {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn clean_except(dir: &Path, keep: Option<&str>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if Some(name.as_str()) != keep {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn trim_dir(dir: &Path, max: u64, keep: Option<&str>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, SystemTime, String)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((e.path(), m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH), e.file_name().to_string_lossy().into_owned()))
        })
        .collect();
    let total: u64 = files.iter().map(|f| f.1).sum();
    if total <= max {
        return;
    }
    files.sort_by_key(|f| f.2);
    let mut excess = total - max;
    for (path, len, _, name) in files {
        if excess == 0 {
            break;
        }
        if Some(name.as_str()) == keep {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            excess = excess.saturating_sub(len);
        }
    }
}

fn cache_home() -> PathBuf {
    match std::env::var("XDG_CACHE_HOME") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"),
    }
    .join("melissm")
}

fn work_dir(shm: bool) -> PathBuf {
    if shm && Path::new("/dev/shm").is_dir() {
        PathBuf::from("/dev/shm/melissm")
    } else {
        cache_home().join("work")
    }
}

fn play_dir() -> PathBuf {
    cache_home().join("work/play")
}

impl Cache {
    pub fn new() -> Self {
        let shm = Path::new("/dev/shm").is_dir();
        let root = cache_home();
        let work = work_dir(shm);
        let _ = fs::create_dir_all(root.join("audio"));
        let _ = fs::create_dir_all(root.join("art"));
        let _ = fs::create_dir_all(&work);
        let play = play_dir();
        let _ = fs::create_dir_all(&play);
        prune_tmp(&root.join("audio"));
        prune_tmp(&root.join("art"));
        clean_except(&work.join("play"), None);
        clean_except(&play, None);
        let _ = fs::remove_dir_all(work.join("art"));
        let _ = fs::create_dir_all(work.join("art"));
        let cache = Self { root, shm };
        trim_dir(&cache.root.join("audio"), MAX_AUDIO_CACHE, None);
        cache
    }

    fn audio_path(&self, id: &str) -> PathBuf {
        self.root.join("audio").join(format!("{id}.enc"))
    }

    fn art_path(&self, id: &str) -> PathBuf {
        self.root.join("art").join(format!("{id}.enc"))
    }

    pub fn has_audio(&self, id: &str) -> bool {
        self.audio_path(id).is_file()
    }

    pub fn store_audio(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        let r = crypto::seal_file(&self.audio_path(id), bytes);
        let keep = format!("{id}.enc");
        trim_dir(&self.root.join("audio"), MAX_AUDIO_CACHE, Some(&keep));
        r
    }

    pub fn store_art(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        crypto::seal_file(&self.art_path(id), bytes)
    }

    pub fn art_plain(&self, id: &str) -> Option<String> {
        let bytes = crypto::open_file(&self.art_path(id))?;
        let dir = work_dir(self.shm).join("art");
        let _ = fs::create_dir_all(&dir);
        let name = format!("{id}.img");
        let p = dir.join(&name);
        fs::write(&p, bytes).ok()?;
        clean_except(&dir, Some(&name));
        Some(p.to_string_lossy().into_owned())
    }

    pub fn audio_plain(&self, id: &str) -> Option<String> {
        let bytes = crypto::open_file(&self.audio_path(id))?;
        let dir = play_dir();
        let _ = fs::create_dir_all(&dir);
        let p = dir.join(id);
        fs::write(&p, bytes).ok()?;
        Some(p.to_string_lossy().into_owned())
    }

    pub fn drop_plain(&self, keep: Option<&str>) {
        clean_except(&play_dir(), keep);
        let art = keep.map(|id| format!("{id}.img"));
        clean_except(&work_dir(self.shm).join("art"), art.as_deref());
    }

    pub fn size(&self) -> u64 {
        [self.root.join("audio"), self.root.join("art")]
            .iter()
            .filter_map(|d| fs::read_dir(d).ok())
            .flat_map(|rd| rd.flatten())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }

    pub fn clear(&self) -> u64 {
        let size = self.size();
        for sub in ["audio", "art"] {
            if let Ok(rd) = fs::read_dir(self.root.join(sub)) {
                for e in rd.flatten() {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        size
    }
}
