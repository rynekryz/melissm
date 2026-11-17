use crate::crypto;
use crate::innertube::Track;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
    dirty: AtomicBool,
    last_ts: AtomicI64,
    work: PathBuf,
}

fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    base.join("melissm")
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct PlaylistMeta {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

fn tcols() -> &'static str {
    "id, title, artist, album, duration, thumbnail, artist_id, is_album"
}

fn row_track(r: &rusqlite::Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        album: r.get(3)?,
        duration: r.get(4)?,
        thumbnail: r.get(5)?,
        artist_id: r.get(6)?,
        is_album: r.get::<_, i64>(7)? != 0,
        is_artist: false,
        is_playlist: false,
    })
}

impl Store {
    pub fn new() -> Self {
        let shm = std::path::Path::new("/dev/shm").is_dir();
        let work = if shm {
            PathBuf::from("/dev/shm/melissm")
        } else {
            state_dir()
        };
        let _ = fs::create_dir_all(&work);
        let _ = fs::create_dir_all(state_dir());
        let db = work.join("store.db");
        if !db.is_file() {
            if let Some(blob) = crypto::open_file(&state_dir().join("vault.bin")) {
                let _ = fs::write(&db, blob);
            }
        }
        let conn = Connection::open(&db).expect("open store");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS likes(
                pos INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL, title TEXT, artist TEXT,
                album TEXT, duration REAL, thumbnail TEXT, artist_id TEXT, is_album INTEGER);
            CREATE TABLE IF NOT EXISTS history(
                played_at INTEGER PRIMARY KEY, id TEXT, title TEXT, artist TEXT, album TEXT,
                duration REAL, thumbnail TEXT, artist_id TEXT, is_album INTEGER);
            CREATE TABLE IF NOT EXISTS playlists(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, created_at INTEGER);
            CREATE TABLE IF NOT EXISTS playlist_tracks(
                pid INTEGER, pos INTEGER, id TEXT, title TEXT, artist TEXT, album TEXT,
                duration REAL, thumbnail TEXT, artist_id TEXT, is_album INTEGER, PRIMARY KEY(pid,pos));
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT);
            CREATE TABLE IF NOT EXISTS lyrics(id TEXT PRIMARY KEY, text TEXT NOT NULL, updated_at INTEGER);",
        )
        .expect("init store");
        Self { conn: Mutex::new(conn), dirty: AtomicBool::new(false), last_ts: AtomicI64::new(0), work }
    }

    pub fn touch(&self) {
        self.dirty.store(true, Ordering::SeqCst);
    }

    pub fn save(&self) {
        let conn = self.conn.lock().unwrap();
        let snap = self.work.join("snap.db");
        let _ = fs::remove_file(&snap);
        if conn.execute("VACUUM INTO ?", params![snap.to_string_lossy()]).is_ok() {
            if let Ok(bytes) = fs::read(&snap) {
                let _ = fs::create_dir_all(state_dir());
                let _ = crypto::seal_file(&state_dir().join("vault.bin"), &bytes);
            }
            let _ = fs::remove_file(&snap);
        }
        self.dirty.store(false, Ordering::SeqCst);
    }

    pub fn spawn_saver(self: std::sync::Arc<Self>) {
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if self.dirty.load(Ordering::SeqCst) {
                self.save();
            }
        });
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM settings WHERE key = ?", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let _ = self.conn.lock().unwrap().execute(
            "INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
        self.touch();
    }

    pub fn likes(&self) -> Vec<Track> {
        self.conn
            .lock()
            .unwrap()
            .prepare(&format!("SELECT {} FROM likes ORDER BY pos DESC", tcols()))
            .and_then(|mut s| s.query_map([], row_track)?.collect())
            .unwrap_or_default()
    }

    pub fn like_toggle(&self, t: &Track) -> bool {
        let conn = self.conn.lock().unwrap();
        let liked: Option<i64> = conn
            .query_row("SELECT 1 FROM likes WHERE id = ?", params![t.id], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        let liked = if liked.is_some() {
            let _ = conn.execute("DELETE FROM likes WHERE id = ?", params![t.id]);
            false
        } else {
            let min: Option<i64> = conn.query_row("SELECT MIN(pos) FROM likes", [], |r| r.get(0)).ok();
            let pos = min.unwrap_or(0) - 1;
            let _ = conn.execute(
                "INSERT OR IGNORE INTO likes(pos, id, title, artist, album, duration, thumbnail, artist_id, is_album)
                 VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![pos, t.id, t.title, t.artist, t.album, t.duration, t.thumbnail, t.artist_id, t.is_album as i64],
            );
            true
        };
        drop(conn);
        self.touch();
        liked
    }

    pub fn likes_clear(&self) {
        let _ = self.conn.lock().unwrap().execute("DELETE FROM likes", []);
        self.touch();
    }

    pub fn history(&self) -> Vec<Track> {
        self.conn
            .lock()
            .unwrap()
            .prepare(&format!("SELECT {} FROM history ORDER BY played_at DESC LIMIT 200", tcols()))
            .and_then(|mut s| s.query_map([], row_track)?.collect())
            .unwrap_or_default()
    }

    pub fn history_push(&self, t: &Track) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
        let mut prev = self.last_ts.load(Ordering::SeqCst);
        let mut ts = now.max(prev + 1);
        while let Err(cur) = self.last_ts.compare_exchange(prev, ts, Ordering::SeqCst, Ordering::SeqCst) {
            prev = cur;
            ts = now.max(prev + 1);
        }
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM history WHERE id = ?", params![t.id]);
        let _ = conn.execute(
            "INSERT OR REPLACE INTO history(played_at, id, title, artist, album, duration, thumbnail, artist_id, is_album)
             VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                ts,
                t.id, t.title, t.artist, t.album, t.duration, t.thumbnail, t.artist_id, t.is_album as i64
            ],
        );
        let _ = conn.execute(
            "DELETE FROM history WHERE played_at NOT IN (SELECT played_at FROM history ORDER BY played_at DESC LIMIT 200)",
            [],
        );
        drop(conn);
        self.touch();
    }

    pub fn history_remove(&self, id: &str) {
        let _ = self.conn.lock().unwrap().execute("DELETE FROM history WHERE id = ?", params![id]);
        self.touch();
    }

    pub fn history_clear(&self) {
        let _ = self.conn.lock().unwrap().execute("DELETE FROM history", []);
        self.touch();
    }

    pub fn import(&self, likes: &[Track], history: &[Track]) {
        for t in likes {
            let _ = self.conn.lock().unwrap().execute(
                "INSERT OR IGNORE INTO likes(id, title, artist, album, duration, thumbnail, artist_id, is_album)
                 VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
                params![t.id, t.title, t.artist, t.album, t.duration, t.thumbnail, t.artist_id, t.is_album as i64],
            );
        }
        for t in history {
            self.history_push(t);
        }
    }

    pub fn playlists(&self) -> Vec<PlaylistMeta> {
        self.conn
            .lock()
            .unwrap()
            .prepare(
                "SELECT p.id, p.name, COUNT(t.id) FROM playlists p
                 LEFT JOIN playlist_tracks t ON t.pid = p.id GROUP BY p.id ORDER BY p.created_at DESC",
            )
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok(PlaylistMeta { id: r.get(0)?, name: r.get(1)?, count: r.get(2)? })
                })?
                .collect()
            })
            .unwrap_or_default()
    }

    pub fn playlist_create(&self, name: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO playlists(name, created_at) VALUES(?, ?)",
            params![
                name,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
            ],
        );
        let id = conn.last_insert_rowid();
        drop(conn);
        self.touch();
        id
    }

    pub fn playlist_rename(&self, id: i64, name: &str) {
        let _ = self.conn.lock().unwrap().execute("UPDATE playlists SET name = ? WHERE id = ?", params![name, id]);
        self.touch();
    }

    pub fn playlist_delete(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM playlists WHERE id = ?", params![id]);
        let _ = conn.execute("DELETE FROM playlist_tracks WHERE pid = ?", params![id]);
        drop(conn);
        self.touch();
    }

    pub fn playlist_tracks(&self, pid: i64) -> Vec<Track> {
        self.conn
            .lock()
            .unwrap()
            .prepare(&format!("SELECT {} FROM playlist_tracks WHERE pid = ? ORDER BY pos", tcols()))
            .and_then(|mut s| s.query_map(params![pid], row_track)?.collect())
            .unwrap_or_default()
    }

    pub fn playlist_add(&self, pid: i64, t: &Track, after_id: Option<&str>) {
        let conn = self.conn.lock().unwrap();
        let pos = match after_id {
            Some(aid) => conn
                .query_row("SELECT pos FROM playlist_tracks WHERE pid = ? AND id = ?", params![pid, aid], |r| r.get::<_, i64>(0))
                .optional()
                .ok()
                .flatten()
                .map(|p| p + 1),
            None => None,
        };
        let pos = match pos {
            Some(p) => {
                let _ = conn.execute(
                    "UPDATE playlist_tracks SET pos = pos + 1 WHERE pid = ? AND pos >= ?",
                    params![pid, p],
                );
                p
            }
            None => conn
                .query_row("SELECT COALESCE(MAX(pos), 0) + 1 FROM playlist_tracks WHERE pid = ?", params![pid], |r| r.get(0))
                .unwrap_or(1),
        };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO playlist_tracks(pid, pos, id, title, artist, album, duration, thumbnail, artist_id, is_album)
             VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![pid, pos, t.id, t.title, t.artist, t.album, t.duration, t.thumbnail, t.artist_id, t.is_album as i64],
        );
        drop(conn);
        self.touch();
    }

    pub fn playlist_remove(&self, pid: i64, track_id: &str) {
        let conn = self.conn.lock().unwrap();
        let pos: Option<i64> = conn
            .query_row("SELECT pos FROM playlist_tracks WHERE pid = ? AND id = ?", params![pid, track_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        if let Some(p) = pos {
            let _ = conn.execute("DELETE FROM playlist_tracks WHERE pid = ? AND pos = ?", params![pid, p]);
            let _ = conn.execute(
                "UPDATE playlist_tracks SET pos = pos - 1 WHERE pid = ? AND pos > ?",
                params![pid, p],
            );
        }
        drop(conn);
        self.touch();
    }

    pub fn custom_lyrics(&self, id: &str) -> Option<String> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT text FROM lyrics WHERE id = ?", params![id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_custom_lyrics(&self, id: &str, text: &str) {
        let _ = self.conn.lock().unwrap().execute(
            "INSERT INTO lyrics(id, text, updated_at) VALUES(?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET text = excluded.text, updated_at = excluded.updated_at",
            params![id, text, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64],
        );
        self.touch();
    }

    pub fn delete_custom_lyrics(&self, id: &str) {
        let _ = self.conn.lock().unwrap().execute("DELETE FROM lyrics WHERE id = ?", params![id]);
        self.touch();
    }
}
