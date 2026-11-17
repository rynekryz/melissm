use crate::crypto;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Cache {
    root: PathBuf,
    shm: bool,
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

impl Cache {
    pub fn new() -> Self {
        let shm = Path::new("/dev/shm").is_dir();
        let root = cache_home();
        let _ = fs::create_dir_all(root.join("audio"));
        let _ = fs::create_dir_all(root.join("art"));
        let _ = fs::create_dir_all(work_dir(shm));
        Self { root, shm }
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
        crypto::seal_file(&self.audio_path(id), bytes)
    }

    pub fn store_art(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        crypto::seal_file(&self.art_path(id), bytes)
    }

    pub fn art_plain(&self, id: &str) -> Option<String> {
        let bytes = crypto::open_file(&self.art_path(id))?;
        let dir = work_dir(self.shm).join("art");
        let _ = fs::create_dir_all(&dir);
        let p = dir.join(format!("{id}.img"));
        fs::write(&p, bytes).ok()?;
        Some(p.to_string_lossy().into_owned())
    }

    pub fn audio_plain(&self, id: &str) -> Option<String> {
        let bytes = crypto::open_file(&self.audio_path(id))?;
        let dir = work_dir(self.shm).join("play");
        let _ = fs::create_dir_all(&dir);
        let p = dir.join(id);
        fs::write(&p, bytes).ok()?;
        Some(p.to_string_lossy().into_owned())
    }

    pub fn drop_plain(&self, keep: Option<&str>) {
        let dir = work_dir(self.shm).join("play");
        let Ok(rd) = fs::read_dir(&dir) else {
            return;
        };
        for e in rd.flatten() {
            if Some(e.file_name().to_string_lossy().into_owned().as_str()) != keep {
                let _ = fs::remove_file(e.path());
            }
        }
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
