use crate::innertube::Track;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Repeat::Off => "off",
            Repeat::All => "all",
            Repeat::One => "one",
        }
    }
}

pub struct QueueState {
    pub tracks: Vec<Track>,
    pub index: isize,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub prev: Vec<usize>,
    pub expected_next: Option<(u64, usize)>,
    pub loading: bool,
}

impl Default for QueueState {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            index: -1,
            shuffle: false,
            repeat: Repeat::Off,
            prev: Vec::new(),
            expected_next: None,
            loading: false,
        }
    }
}

impl QueueState {
    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(self.index.max(0) as usize)
    }

    fn random_other(&self, cur: usize) -> Option<usize> {
        if self.tracks.len() < 2 {
            return None;
        }
        let len = self.tracks.len();
        let mut buf = [0u8; 4];
        let _ = getrandom::getrandom(&mut buf);
        let n = u32::from_ne_bytes(buf) as usize % (len - 1);
        Some(if n >= cur { n + 1 } else { n })
    }

    pub fn peek_next(&self, cur: usize) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.repeat == Repeat::One {
            return Some(cur);
        }
        if self.shuffle {
            return self.random_other(cur);
        }
        if cur + 1 < self.tracks.len() {
            return Some(cur + 1);
        }
        if self.repeat == Repeat::All {
            return Some(0);
        }
        None
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tracks: self.tracks.clone(),
            index: self.index,
            shuffle: self.shuffle,
            repeat: self.repeat.as_str().to_string(),
            loading: self.loading,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct Snapshot {
    pub tracks: Vec<Track>,
    pub index: isize,
    pub shuffle: bool,
    pub repeat: String,
    pub loading: bool,
}
