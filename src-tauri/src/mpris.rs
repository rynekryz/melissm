use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter, Manager};
use zbus::{
    connection::Builder,
    object_server::{InterfaceRef, SignalEmitter},
    zvariant::{ObjectPath, OwnedValue, Value},
};


const PATH: &str = "/org/mpris/MediaPlayer2";
const NAME: &str = "org.mpris.MediaPlayer2.melissm";

#[derive(Default, Clone)]
pub struct MprisData {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
    pub track: u64,
    pub playing: bool,
    pub position: f64,
    pub pos_at: Option<Instant>,
    pub duration: f64,
    pub shuffle: bool,
    pub repeat: String,
    pub volume: f64,
}

impl MprisData {
    pub fn current_position(&self) -> f64 {
        if self.playing && self.pos_at.is_some() {
            self.position + self.pos_at.unwrap().elapsed().as_secs_f64()
        } else {
            self.position
        }
    }

    pub fn loop_status(&self) -> &'static str {
        match self.repeat.as_str() {
            "all" => "Playlist",
            "one" => "Track",
            _ => "None",
        }
    }
}

static DATA: Mutex<Option<MprisData>> = Mutex::new(None);
static CONN: Mutex<Option<zbus::Connection>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

fn get() -> MprisData {
    DATA.lock().unwrap().clone().unwrap_or_default()
}

fn set(f: impl FnOnce(&mut MprisData)) -> bool {
    if !STARTED.load(Ordering::SeqCst) {
        return false;
    }
    let mut g = DATA.lock().unwrap();
    if g.is_none() {
        *g = Some(MprisData::default());
    }
    let d = g.as_mut().unwrap();
    f(d);
    true
}

fn session() -> Option<zbus::Connection> {
    CONN.lock().unwrap().clone()
}

fn notify(props: Vec<&'static str>) {
    tauri::async_runtime::spawn(async move {
        let conn = match session() {
            Some(c) => c,
            None => return,
        };
        let iface = match conn.object_server().interface::<_, Player>(PATH).await {
            Ok(i) => i,
            Err(_) => return,
        };
        let mut changed: HashMap<&str, Value> = HashMap::new();
        for p in props {
            if let Some(v) = prop(p) {
                changed.insert(p.into(), v);
            }
        }
        if changed.is_empty() {
            return;
        }
        let _ = zbus::fdo::Properties::properties_changed(
            iface.signal_emitter(),
            "org.mpris.MediaPlayer2.Player".try_into().unwrap(),
            changed,
            Vec::new().into(),
        )
        .await;
    });
}

fn prop(name: &'static str) -> Option<Value<'static>> {
    let d = get();
    Some(match name {
        "PlaybackStatus" => {
            if d.title.is_empty() {
                "Stopped"
            } else if d.playing {
                "Playing"
            } else {
                "Paused"
            }
        }
        .into(),
        "LoopStatus" => d.loop_status().into(),
        "Metadata" => Value::from(metadata_map(&d)),
        "Position" => ((d.current_position() * 1_000_000.0) as i64).into(),
        "Shuffle" => d.shuffle.into(),
        "Volume" => d.volume.into(),
        _ => return None,
    })
}

pub fn start(app: AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let player = Player { app: app.clone() };
        let root = Root { app };
        match Builder::session()
            .unwrap()
            .name(NAME)
            .unwrap()
            .serve_at(PATH, root)
            .unwrap()
            .serve_at(PATH, player)
            .unwrap()
            .build()
            .await
        {
            Ok(c) => *CONN.lock().unwrap() = Some(c),
            Err(e) => {
                eprintln!("mpris: {e}");
                STARTED.store(false, Ordering::SeqCst);
            }
        }
    });
}

pub fn update_track(title: String, artist: String, album: String, art: String) {
    let old = get();
    let changed = old.title != title;
    let was_playing = old.playing;
    if !set(|d| {
        if changed {
            d.track += 1;
            d.position = 0.0;
            d.pos_at = Some(Instant::now());
        }
        d.title = title;
        d.artist = artist;
        d.album = album;
        d.art = art;
        if d.title.is_empty() {
            d.playing = false;
        }
    }) {
        return;
    }
    if changed {
        if !old.title.is_empty() {
            d_seeked(0.0);
        }
        notify(vec!["Metadata"]);
    }
    if !get().playing && was_playing {
        notify(vec!["PlaybackStatus"]);
    }
}

pub fn update_status(playing: bool, position: f64, duration: f64) {
    let old_pos = get().current_position();
    let jump = (position - old_pos).abs() > 1.5;
    let was = get().playing;
    if !set(|d| {
        d.playing = playing;
        d.duration = duration;
        d.position = position;
        d.pos_at = Some(Instant::now());
    }) {
        return;
    }
    if was != playing {
        notify(vec!["PlaybackStatus"]);
    }
    if jump {
        d_seeked(position);
    }
}

pub fn update_flags(shuffle: bool, repeat: String) {
    if !set(|d| {
        d.shuffle = shuffle;
        d.repeat = repeat;
    }) {
        return;
    }
    notify(vec!["Shuffle", "LoopStatus"]);
}

pub fn update_volume(volume: f64) {
    if !set(|d| d.volume = volume) {
        return;
    }
    notify(vec!["Volume"]);
}

fn d_seeked(position: f64) {
    tauri::async_runtime::spawn(async move {
        let conn = match session() {
            Some(c) => c,
            None => return,
        };
        let iface: InterfaceRef<Player> = match conn.object_server().interface(PATH).await {
            Ok(i) => i,
            Err(_) => return,
        };
        let _ = Player::seeked(iface.signal_emitter(), (position * 1_000_000.0) as i64).await;
    });
}

fn metadata_map(d: &MprisData) -> HashMap<String, OwnedValue> {
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    if d.title.is_empty() {
        return m;
    }
    let trackid = format!("/dev/ryne/melissm/track{}", d.track);
    if let Ok(p) = ObjectPath::try_from(trackid.as_str()) {
        m.insert("mpris:trackid".into(), Value::from(p).try_into().unwrap());
    }
    m.insert("mpris:length".into(), Value::from((d.duration * 1_000_000.0) as i64).try_into().unwrap());
    if !d.art.is_empty() {
        m.insert("mpris:artUrl".into(), Value::from(d.art.clone()).try_into().unwrap());
    }
    m.insert("xesam:title".into(), Value::from(d.title.clone()).try_into().unwrap());
    m.insert("xesam:artist".into(), Value::from(vec![d.artist.clone()]).try_into().unwrap());
    if !d.album.is_empty() {
        m.insert("xesam:album".into(), Value::from(d.album.clone()).try_into().unwrap());
    }
    m
}

struct Root {
    app: AppHandle,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn quit(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    fn raise(&self) -> zbus::fdo::Result<()> {
        if let Some(w) = self.app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        "Melissm".into()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "melissm".into()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    app: AppHandle,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    fn play(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "play".to_string()).ok();
        Ok(())
    }

    fn pause(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "pause".to_string()).ok();
        Ok(())
    }

    fn play_pause(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "play-pause".to_string()).ok();
        Ok(())
    }

    fn stop(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "pause".to_string()).ok();
        Ok(())
    }

    fn next(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "next".to_string()).ok();
        Ok(())
    }

    fn previous(&self) -> zbus::fdo::Result<()> {
        self.app.emit("tray-action", "previous".to_string()).ok();
        Ok(())
    }

    fn seek(&self, offset: i64) -> zbus::fdo::Result<()> {
        self.app
            .emit("tray-action", format!("seek:{}", offset as f64 / 1_000_000.0))
            .ok();
        Ok(())
    }

    fn set_position(&self, _trackid: ObjectPath<'_>, position: i64) -> zbus::fdo::Result<()> {
        self.app
            .emit("tray-action", format!("seek:{}", position as f64 / 1_000_000.0))
            .ok();
        Ok(())
    }

    fn open_uri(&self, _uri: String) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let d = get();
        if d.title.is_empty() {
            "Stopped".into()
        } else if d.playing {
            "Playing".into()
        } else {
            "Paused".into()
        }
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        get().loop_status().into()
    }

    #[zbus(property)]
    fn set_loop_status(&mut self, value: String) {
        if value != get().loop_status() {
            self.app.emit("tray-action", "repeat".to_string()).ok();
        }
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        get().shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&mut self, value: bool) {
        if value != get().shuffle {
            self.app.emit("tray-action", "shuffle".to_string()).ok();
        }
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata_map(&get())
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        get().volume
    }

    #[zbus(property)]
    fn set_volume(&mut self, value: f64) {
        self.app.emit("tray-action", format!("volume:{value}")).ok();
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        (get().current_position() * 1_000_000.0) as i64
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}
