mod audio;
mod crypto;
mod cache;
mod innertube;
mod log;
mod lyrics;
#[cfg(target_os = "linux")]
mod mpris;
mod queue;
mod store;
mod theming;

use audio::{Cmd, Event, Initial, Player};
use cache::Cache;
use innertube::{Track, Yt};
use lyrics::Lyrics;
use queue::{QueueState, Repeat, Snapshot};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    sync::Mutex,
};
use store::Store;
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem},
    tray::TrayIcon,
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WindowEvent, Wry,
};
use theming::{SharedOpts, ThemeOpts};

const MAX_AUTO_QUEUE: usize = 1000;

struct AppState {
    yt: Yt,
    player: Player,
    store: std::sync::Arc<Store>,
    cache: Cache,
    info: Mutex<PlayerInfo>,
    tray: Mutex<Option<TrayIcon>>,
    queue: Mutex<QueueState>,
    position: Mutex<f64>,
    quality: Mutex<String>,
    tags: AtomicU64,
    preload_gen: AtomicU64,
    queue_gen: AtomicU64,
}

#[derive(Clone, Serialize, Default)]
struct PlayerInfo {
    playing: bool,
    shuffle: bool,
    repeat: String,
    title: String,
    artist: String,
}

fn tray_menu(app: &AppHandle, info: &PlayerInfo) -> tauri::Result<Menu<Wry>> {
    let now = if info.title.is_empty() {
        "Nothing playing".to_string()
    } else if info.artist.is_empty() {
        format!("Now playing: {}", info.title)
    } else {
        format!("Now playing: {} - {}", info.title, info.artist)
    };
    let now = MenuItem::with_id(app, "now", now, false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let previous = MenuItem::with_id(app, "previous", "Previous", true, None::<&str>)?;
    let play = MenuItem::with_id(app, "play-pause", if info.playing { "Pause" } else { "Play" }, true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Next", true, None::<&str>)?;
    let shuffle = CheckMenuItem::with_id(app, "shuffle", "Shuffle", true, info.shuffle, None::<&str>)?;
    let repeat_label = match info.repeat.as_str() {
        "all" => "Repeat: all",
        "one" => "Repeat: one",
        _ => "Repeat: off",
    };
    let repeat = MenuItem::with_id(app, "repeat", repeat_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let items: Vec<&dyn IsMenuItem<Wry>> =
        vec![&now, &show, &previous, &play, &next, &shuffle, &repeat, &quit];
    Menu::with_items(app, &items)
}

fn refresh_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let info = state.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut tray = state.tray.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(tray) = tray.as_mut() {
        if let Ok(menu) = tray_menu(app, &info) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn emit_queue(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snap = state.queue.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
    let _ = app.emit("queue-state", snap);
}

fn push_flags(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (shuffle, repeat) = {
        let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        (q.shuffle, q.repeat.as_str().to_string())
    };
    {
        let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        info.shuffle = shuffle;
        info.repeat = repeat.clone();
    }
    refresh_tray(app);
    #[cfg(target_os = "linux")]
    mpris::update_flags(shuffle, repeat);
}

fn update_now_playing(app: &AppHandle, t: &Track) {
    let state = app.state::<AppState>();
    {
        let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
        info.title = t.title.clone();
        info.artist = t.artist.clone();
    }
    refresh_tray(app);
    #[cfg(target_os = "linux")]
    mpris::update_track(t.title.clone(), t.artist.clone(), t.album.clone(), t.thumbnail.clone());
}

fn update_artwork(app: AppHandle, t: Track) {
    let opts: State<SharedOpts> = app.state();
    let source_artwork = matches!(opts.lock().unwrap_or_else(|e| e.into_inner()).source, theming::Source::Artwork);
    if !source_artwork {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let id = t.id.clone();
        let thumb = t.thumbnail.clone();
        let path = if let Some(p) = state.cache.art_plain(&id) {
            Some(p)
        } else {
            if thumb.is_empty() {
                None
            } else {
                match state.yt.http.get(&thumb).send().await {
                    Ok(r) if r.status().is_success() => match r.bytes().await {
                        Ok(bytes) => {
                            let _ = state.cache.store_art(&id, &bytes);
                            state.cache.art_plain(&id)
                        }
                        Err(_) => None,
                    },
                    _ => None,
                }
            }
        };
        theming::set_artwork(path);
        let opts: State<SharedOpts> = app.state();
        let snapshot = opts.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(palette) = tauri::async_runtime::spawn_blocking(move || theming::current(&snapshot))
            .await
            .ok()
            .flatten()
        {
            let _ = app.emit("theme-changed", palette);
        }
    });
}

async fn resolve_url(state: &AppState, id: &str, quality: &str) -> Result<(String, f64, String), String> {
    if state.cache.has_audio(id) {
        if let Some(path) = state.cache.audio_plain(id) {
            return Ok((format!("file://{path}"), 0.0, String::new()));
        }
    }
    let (url, dur, ua) = state.yt.resolve_stream(id, quality).await?;
    Ok((url, dur, ua.to_string()))
}

fn cache_track(http: reqwest::Client, cache: Cache, id: String, url: String) {
    if url.starts_with("file:") {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Ok(r) = http.get(&url).send().await {
            if r.status().is_success() {
                if let Ok(bytes) = r.bytes().await {
                    let _ = cache.store_audio(&id, &bytes);
                }
            }
        }
    });
}

async fn start_index(app: AppHandle, i: usize, push_history: bool) {
    let state = app.state::<AppState>();
    let gen = state.queue_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let t = {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        let t = q.tracks.get(i).cloned();
        if t.is_some() {
            if push_history && q.index >= 0 && q.index != i as isize {
                let prev = q.index as usize;
                q.prev.push(prev);
                if q.prev.len() > 200 {
                    q.prev.remove(0);
                }
            }
            q.index = i as isize;
            q.expected_next = None;
            q.loading = true;
        }
        t
    };
    let Some(t) = t else { return; };
    emit_queue(&app);
    let quality = state.quality.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let resolved = resolve_url(&state, &t.id, &quality).await;
    if state.queue_gen.load(Ordering::SeqCst) != gen {
        return;
    }
    let (url, dur, ua) = match resolved {
        Ok(r) => r,
        Err(e) => {
            if state.queue_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            log::line(&format!("resolve failed for {}: {e}", t.id));
            let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.loading = false;
            drop(q);
            emit_queue(&app);
            let _ = app.emit("playback-failed", e);
            return;
        }
    };
    let duration = if dur > 0.0 { dur } else { t.duration };
    state.player.send(Cmd::Load { url: url.clone(), duration, ua: ua.clone() });
    if url.starts_with("file:") {
        state.cache.drop_plain(Some(&t.id));
    } else if matches!(state.store.setting("autocache").as_deref(), Some("1") | None)
        && !state.cache.has_audio(&t.id)
    {
        cache_track(state.yt.http.clone(), state.cache.clone(), t.id.clone(), url.clone());
    }
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.loading = false;
    }
    emit_queue(&app);
    update_now_playing(&app, &t);
    state.store.history_push(&t);
    update_artwork(app.clone(), t.clone());
    preload_next(app.clone()).await;
}

async fn adopt_index(app: AppHandle, i: usize, push_history: bool) {
    let state = app.state::<AppState>();
    let t = {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        let t = q.tracks.get(i).cloned();
        if t.is_some() {
            if push_history && q.index >= 0 && q.index != i as isize {
                let prev = q.index as usize;
                q.prev.push(prev);
                if q.prev.len() > 200 {
                    q.prev.remove(0);
                }
            }
            q.index = i as isize;
            q.expected_next = None;
            q.loading = false;
        }
        t
    };
    let Some(t) = t else { return };
    emit_queue(&app);
    update_now_playing(&app, &t);
    state.store.history_push(&t);
    update_artwork(app.clone(), t.clone());
    preload_next(app.clone()).await;
}

async fn extend_radio(app: AppHandle) -> bool {
    let state = app.state::<AppState>();
    let (cur, id) = {
        let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        match q.current() {
            Some(t) => (q.index, t.id.clone()),
            None => return false,
        }
    };
    let Ok(related) = state.yt.radio(&id).await else {
        return false;
    };
    let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
    let room = MAX_AUTO_QUEUE.saturating_sub(q.tracks.len());
    let fresh: Vec<Track> = related
        .into_iter()
        .filter(|r| !q.tracks.iter().any(|t| t.id == r.id))
        .take(25.min(room))
        .collect();
    let added = fresh.len();
    q.tracks.extend(fresh);
    drop(q);
    if added == 0 {
        return false;
    }
    let _ = cur;
    emit_queue(&app);
    true
}

async fn advance(app: AppHandle, force: Option<usize>, manual: bool) {
    let state = app.state::<AppState>();
    let cur = {
        let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        if q.loading || q.tracks.is_empty() {
            return;
        }
        q.index
    };
    let next = match force {
        Some(i) => Some(i),
        None => {
            let peek = {
                let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
                if manual {
                    q.peek_manual_next(cur.max(0) as usize)
                } else {
                    q.peek_next(cur.max(0) as usize)
                }
            };
            peek.or_else(|| None::<usize>)
        }
    };
    let autoload = state.store.setting("autoload").as_deref() != Some("0");
    let next = match next {
        Some(i) => Some(i),
        None => {
            let (last, repeat, src) = {
                let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
                (cur as usize >= q.tracks.len().saturating_sub(1), q.repeat, q.source.clone())
            };
            if last && repeat == Repeat::Off {
                if src.is_some() {
                    if autoload && load_more_playlist(app.clone()).await {
                        Some(cur as usize + 1)
                    } else {
                        None
                    }
                } else if extend_radio(app.clone()).await {
                    Some(cur as usize + 1)
                } else {
                    None
                }
            } else {
                None
            }
        }
    };
    let Some(next) = next else {
        state.player.send(Cmd::Stop);
        {
            let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
            info.playing = false;
        }
        refresh_tray(&app);
        return;
    };
    start_index(app.clone(), next, true).await;
}

async fn load_more_playlist(app: AppHandle) -> bool {
    let state = app.state::<AppState>();
    let cont = state.queue.lock().unwrap_or_else(|e| e.into_inner()).cont.clone();
    let Some(token) = cont else { return false };
    let (fresh, next) = match state.yt.album_more(&token).await {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
    let room = MAX_AUTO_QUEUE.saturating_sub(q.tracks.len());
    let added: Vec<Track> = fresh
        .into_iter()
        .filter(|r| !q.tracks.iter().any(|t| t.id == r.id))
        .take(50.min(room))
        .collect();
    let added_n = added.len();
    q.tracks.extend(added);
    q.cont = next;
    drop(q);
    emit_queue(&app);
    added_n > 0
}

async fn handle_ended(app: AppHandle, tag: u64, gapless: bool) {
    let state = app.state::<AppState>();
    let (cur, repeat, expected) = {
        let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        (q.index, q.repeat, q.expected_next)
    };
    if repeat == Repeat::One {
        start_index(app.clone(), cur.max(0) as usize, false).await;
        return;
    }
    if gapless {
        if let Some((t, i)) = expected {
            if t == tag && (i as isize) != cur {
                adopt_index(app.clone(), i, true).await;
                return;
            }
        }
        advance(app, None, false).await;
        return;
    }
    advance(app, None, false).await;
}

async fn preload_next(app: AppHandle) {
    let state = app.state::<AppState>();
    let gen = state.preload_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let next = {
        let q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        let cur = q.index;
        if cur < 0 {
            state.player.send(Cmd::CancelPreload);
            return;
        }
        q.peek_next(cur as usize).map(|i| {
            let tag = state.tags.fetch_add(1, Ordering::SeqCst) + 1;
            (cur, tag, i)
        })
    };
    let Some((cur, tag, idx)) = next else {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.expected_next = None;
        state.player.send(Cmd::CancelPreload);
        return;
    };
    let id = state.queue.lock().unwrap_or_else(|e| e.into_inner()).tracks.get(idx).map(|t| t.id.clone());
    let Some(id) = id else { return };
    let quality = state.quality.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Ok((url, _dur, ua)) = resolve_url(&state, &id, &quality).await else {
        if state.preload_gen.load(Ordering::SeqCst) == gen {
            let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.expected_next = None;
            state.player.send(Cmd::CancelPreload);
        }
        return;
    };
    if state.preload_gen.load(Ordering::SeqCst) != gen {
        return;
    }
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        if q.index != cur {
            return;
        }
        q.expected_next = Some((tag, idx));
    }
    state.player.send(Cmd::Preload { url, ua, tag });
}

fn skip_prev(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pos = *state.position.lock().unwrap_or_else(|e| e.into_inner());
    if pos > 4.0 {
        state.player.send(Cmd::Seek(0.0));
        return;
    }
    let target = {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        let cur = q.index;
        q.expected_next = None;
        let mut found = None;
        while let Some(i) = q.prev.pop() {
            if (i as isize) != cur {
                found = Some(i as isize);
                break;
            }
        }
        found.or_else(|| {
            if q.shuffle {
                None
            } else if cur > 0 {
                Some(cur - 1)
            } else if q.repeat == Repeat::All && !q.tracks.is_empty() {
                Some(q.tracks.len() as isize - 1)
            } else {
                None
            }
        })
    };
    if let Some(i) = target {
        tauri::async_runtime::spawn(start_index(app.clone(), i as usize, false));
    }
}

fn toggle_play(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.info.lock().unwrap_or_else(|e| e.into_inner()).playing {
        state.player.send(Cmd::Pause);
    } else {
        state.player.send(Cmd::Resume);
    }
}

fn toggle_shuffle(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.shuffle = !q.shuffle;
        q.expected_next = None;
        state.store.set_setting("shuffle", if q.shuffle { "1" } else { "0" });
    }
    state.player.send(Cmd::CancelPreload);
    push_flags(app);
    emit_queue(&app);
    tauri::async_runtime::spawn(preload_next(app.clone()));
}

fn cycle_repeat(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.repeat = match q.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        q.expected_next = None;
        state.store.set_setting("repeat", q.repeat.as_str());
    }
    state.player.send(Cmd::CancelPreload);
    push_flags(app);
    emit_queue(&app);
    tauri::async_runtime::spawn(preload_next(app.clone()));
}

fn reschedule_preload(app: &AppHandle) {
    tauri::async_runtime::spawn(preload_next(app.clone()));
}

pub fn native_action(app: &AppHandle, action: &str) {
    match action {
        "play" => app.state::<AppState>().player.send(Cmd::Resume),
        "pause" => app.state::<AppState>().player.send(Cmd::Pause),
        "play-pause" => toggle_play(app),
        "next" => {
            let a = app.clone();
            tauri::async_runtime::spawn(async move { advance(a, None, true).await });
        }
        "previous" => skip_prev(app),
        "shuffle" => toggle_shuffle(app),
        "repeat" => cycle_repeat(app),
        s => {
            if let Some(v) = s.strip_prefix("seek:") {
                if let Ok(t) = v.parse::<f64>() {
                    app.state::<AppState>().player.send(Cmd::Seek(t));
                }
            } else if let Some(v) = s.strip_prefix("volume:") {
                if let Ok(x) = v.parse::<f64>() {
                    let x = x.clamp(0.0, 1.0);
                    app.state::<AppState>().player.send(Cmd::Volume(x as f32));
                    app.state::<AppState>().store.set_setting("volume", &x.to_string());
                    #[cfg(target_os = "linux")]
                    mpris::update_volume(x);
                }
            }
        }
    }
}

#[tauri::command]
async fn search(state: State<'_, AppState>, query: String) -> Result<Vec<Track>, String> {
    state.yt.search(&query).await
}

#[tauri::command]
async fn home(state: State<'_, AppState>) -> Result<Vec<innertube::Section>, String> {
    state.yt.home().await
}

#[tauri::command]
async fn playlists_for_you(state: State<'_, AppState>, query: String) -> Result<Vec<innertube::Track>, String> {
    state.yt.playlists(&query).await
}

#[tauri::command]
async fn album(state: State<'_, AppState>, id: String) -> Result<innertube::AlbumPage, String> {
    state.yt.album(&id).await
}

#[tauri::command]
async fn album_more(state: State<'_, AppState>, token: String) -> Result<(Vec<innertube::Track>, Option<String>), String> {
    state.yt.album_more(&token).await
}

#[tauri::command]
async fn artist(state: State<'_, AppState>, id: String) -> Result<innertube::ArtistPage, String> {
    state.yt.artist(&id).await
}

#[tauri::command]
fn get_queue(state: State<'_, AppState>) -> Snapshot {
    state.queue.lock().unwrap_or_else(|e| e.into_inner()).snapshot()
}

#[tauri::command]
async fn play_tracks(
    app: AppHandle,
    tracks: Vec<Track>,
    index: usize,
    src: Option<String>,
    cont: Option<String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    if tracks.is_empty() {
        return Err("empty queue".into());
    }
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.tracks = tracks;
        q.index = -1;
        q.prev.clear();
        q.expected_next = None;
        q.source = src;
        q.cont = cont;
    }
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
    start_index(app.clone(), index, false).await;
    Ok(())
}

#[tauri::command]
async fn play_radio(app: AppHandle, track: Track) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.tracks = vec![track];
        q.index = -1;
        q.prev.clear();
        q.expected_next = None;
        q.source = None;
        q.cont = None;
    }
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
    start_index(app.clone(), 0, false).await;
    tauri::async_runtime::spawn(extend_radio(app.clone()));
    Ok(())
}

#[tauri::command]
async fn play_at(app: AppHandle, index: usize) -> Result<(), String> {
    advance(app.clone(), Some(index), false).await;
    Ok(())
}

#[tauri::command]
async fn skip_next(app: AppHandle) -> Result<(), String> {
    advance(app.clone(), None, true).await;
    Ok(())
}

#[tauri::command]
fn skip_prev_cmd(app: AppHandle) {
    skip_prev(&app);
}

#[tauri::command]
fn toggle_play_cmd(app: AppHandle) {
    toggle_play(&app);
}

#[tauri::command]
fn pause(state: State<'_, AppState>) {
    state.player.send(Cmd::Pause);
}

#[tauri::command]
fn resume(state: State<'_, AppState>) {
    state.player.send(Cmd::Resume);
}

#[tauri::command]
fn seek(state: State<'_, AppState>, seconds: f64) {
    state.player.send(Cmd::Seek(seconds));
}

#[tauri::command]
fn volume(state: State<'_, AppState>, value: f32) {
    let v = value.clamp(0.0, 1.0);
    state.player.send(Cmd::Volume(v));
    state.store.set_setting("volume", &v.to_string());
    #[cfg(target_os = "linux")]
    mpris::update_volume(v as f64);
}

#[tauri::command]
fn toggle_shuffle_cmd(app: AppHandle) {
    toggle_shuffle(&app);
}

#[tauri::command]
fn cycle_repeat_cmd(app: AppHandle) {
    cycle_repeat(&app);
}

#[tauri::command]
fn set_window_visible(state: State<'_, AppState>, visible: bool) {
    state.player.send(Cmd::Visible(visible));
}

#[tauri::command]
fn set_quality(state: State<'_, AppState>, quality: String) {
    *state.quality.lock().unwrap_or_else(|e| e.into_inner()) = quality.clone();
    state.store.set_setting("quality", &quality);
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for key in [
        "volume", "speed", "pitch", "eq", "quality", "crossfade", "shuffle", "repeat", "autocache", "autoload",
        "lyr_lrclib", "lyr_netease", "lyrics_offset", "panel_close",
    ] {
        m.insert(key.to_string(), state.store.setting(key).unwrap_or_default());
    }
    m
}

#[tauri::command]
fn set_speed(state: State<'_, AppState>, value: f64) {
    state.player.set_speed(value);
    state.store.set_setting("speed", &value.to_string());
}

#[tauri::command]
fn set_pitch(state: State<'_, AppState>, value: f64) {
    state.player.set_pitch(value);
    state.store.set_setting("pitch", &value.to_string());
}

#[tauri::command]
fn set_eq(state: State<'_, AppState>, band: u32, gain: f64) {
    if band >= 10 {
        return;
    }
    state.player.set_eq(band, gain);
    let mut eq: Vec<f64> = state
        .store
        .setting("eq")
        .and_then(|v| serde_json::from_str::<Vec<f64>>(&v).ok())
        .unwrap_or_else(|| vec![0.0; 10]);
    if eq.len() != 10 {
        eq = vec![0.0; 10];
    }
    eq[band as usize % 10] = gain;
    state.store.set_setting("eq", &serde_json::to_string(&eq).unwrap_or_default());
}

#[tauri::command]
fn set_crossfade(state: State<'_, AppState>, seconds: u32) {
    state.player.send(Cmd::Fade(seconds.clamp(0, 15)));
    state.store.set_setting("crossfade", &seconds.to_string());
}

#[tauri::command]
fn set_autocache(state: State<'_, AppState>, enabled: bool) {
    state.store.set_setting("autocache", if enabled { "1" } else { "0" });
}

#[tauri::command]
fn set_autoload(state: State<'_, AppState>, enabled: bool) {
    state.store.set_setting("autoload", if enabled { "1" } else { "0" });
}

#[tauri::command]
fn cache_info(state: State<'_, AppState>) -> u64 {
    state.cache.size()
}

#[tauri::command]
fn cache_clear(state: State<'_, AppState>) -> u64 {
    state.cache.clear()
}

#[tauri::command]
async fn add_to_queue(app: AppHandle, track: Track, next: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let empty = {
        let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
        if q.index < 0 || q.tracks.is_empty() {
            q.tracks = vec![track];
            q.index = 0;
            true
        } else {
            let at = if next { (q.index as usize) + 1 } else { q.tracks.len() };
            let at = at.min(q.tracks.len());
            q.tracks.insert(at, track);
            q.expected_next = None;
            false
        }
    };
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
    if empty {
        start_index(app.clone(), 0, false).await;
    } else {
        reschedule_preload(&app);
    }
    Ok(())
}

#[tauri::command]
fn remove_from_queue(app: AppHandle, index: usize) {
    let state = app.state::<AppState>();
    let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
    if index >= q.tracks.len() || index as isize == q.index {
        return;
    }
    q.tracks.remove(index);
    if (index as isize) < q.index {
        q.index -= 1;
    }
    q.prev = q
        .prev
        .iter()
        .filter(|p| **p != index)
        .map(|p| if *p > index { *p - 1 } else { *p })
        .collect();
    q.expected_next = None;
    drop(q);
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
    reschedule_preload(&app);
}

#[tauri::command]
fn move_in_queue(app: AppHandle, from: usize, to: usize) {
    let state = app.state::<AppState>();
    let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
    if from >= q.tracks.len() || to >= q.tracks.len() || from as isize == q.index || to as isize == q.index {
        return;
    }
    let t = q.tracks.remove(from);
    q.tracks.insert(to, t);
    let shift = |i: usize| {
        if from < to && i > from && i <= to {
            i - 1
        } else if from > to && i >= to && i < from {
            i + 1
        } else {
            i
        }
    };
    if q.index >= 0 {
        q.index = shift(q.index as usize) as isize;
    }
    q.prev = q.prev.iter().map(|p| shift(*p)).collect();
    q.expected_next = None;
    drop(q);
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
    reschedule_preload(&app);
}

#[tauri::command]
fn clear_upcoming(app: AppHandle) {
    let state = app.state::<AppState>();
    let mut q = state.queue.lock().unwrap_or_else(|e| e.into_inner());
    if q.index >= 0 {
        let keep = q.index as usize + 1;
        q.tracks.truncate(keep);
    }
    q.expected_next = None;
    drop(q);
    state.player.send(Cmd::CancelPreload);
    emit_queue(&app);
}

#[tauri::command]
fn likes_list(state: State<'_, AppState>) -> Vec<Track> {
    state.store.likes()
}

#[tauri::command]
fn like_toggle(state: State<'_, AppState>, track: Track) -> bool {
    state.store.like_toggle(&track)
}

#[tauri::command]
fn likes_clear(state: State<'_, AppState>) {
    state.store.likes_clear();
}

#[tauri::command]
fn history_list(state: State<'_, AppState>) -> Vec<Track> {
    state.store.history()
}

#[tauri::command]
fn history_remove(state: State<'_, AppState>, id: String) {
    state.store.history_remove(&id);
}

#[tauri::command]
fn history_clear(state: State<'_, AppState>) {
    state.store.history_clear();
}

#[tauri::command]
fn store_import(state: State<'_, AppState>, likes: Vec<Track>, history: Vec<Track>) {
    state.store.import(&likes, &history);
}

#[tauri::command]
fn playlists_list(state: State<'_, AppState>) -> Vec<store::PlaylistMeta> {
    state.store.playlists()
}

#[tauri::command]
fn playlist_create(state: State<'_, AppState>, name: String) -> i64 {
    state.store.playlist_create(&name)
}

#[tauri::command]
fn playlist_rename(state: State<'_, AppState>, id: i64, name: String) {
    state.store.playlist_rename(id, &name);
}

#[tauri::command]
fn playlist_delete(state: State<'_, AppState>, id: i64) {
    state.store.playlist_delete(id);
}

#[tauri::command]
fn playlist_tracks(state: State<'_, AppState>, id: i64) -> Vec<Track> {
    state.store.playlist_tracks(id)
}

#[tauri::command]
fn playlist_add(state: State<'_, AppState>, id: i64, track: Track, after_id: Option<String>) {
    state.store.playlist_add(id, &track, after_id.as_deref());
}

#[tauri::command]
fn playlist_remove(state: State<'_, AppState>, id: i64, track_id: String) {
    state.store.playlist_remove(id, &track_id);
}

#[tauri::command]
async fn lyrics(
    state: State<'_, AppState>,
    id: String,
    title: String,
    artist: String,
    album: String,
    duration: f64,
) -> Result<Lyrics, String> {
    Ok(lyrics::fetch(&state.store, &state.yt.http, &id, &title, &artist, &album, duration).await)
}

#[tauri::command]
fn lyrics_get_custom(state: State<'_, AppState>, id: String) -> Option<String> {
    state.store.custom_lyrics(&id)
}

#[tauri::command]
fn lyrics_save_custom(state: State<'_, AppState>, id: String, text: String) {
    state.store.save_custom_lyrics(&id, &text);
}

#[tauri::command]
fn lyrics_delete_custom(state: State<'_, AppState>, id: String) {
    state.store.delete_custom_lyrics(&id);
}

#[tauri::command]
fn lyrics_set_provider(state: State<'_, AppState>, provider: String, enabled: bool) {
    state.store.set_setting(&format!("lyr_{provider}"), if enabled { "1" } else { "0" });
}

#[tauri::command]
fn lyrics_set_offset(state: State<'_, AppState>, offset: f64) {
    state.store.set_setting("lyrics_offset", &offset.clamp(-10.0, 10.0).to_string());
}

#[tauri::command]
fn set_panel_close(state: State<'_, AppState>, enabled: bool) {
    state.store.set_setting("panel_close", if enabled { "1" } else { "0" });
}

#[tauri::command]
async fn set_theme(opts: State<'_, SharedOpts>, theme: ThemeOpts) -> Result<BTreeMap<String, String>, String> {
    *opts.lock().unwrap_or_else(|e| e.into_inner()) = theme;
    let snapshot = opts.lock().unwrap_or_else(|e| e.into_inner()).clone();
    tauri::async_runtime::spawn_blocking(move || theming::current(&snapshot))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no palette available".to_string())
}

#[tauri::command]
fn system_scale() -> f64 {
    let txt = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "text-scaling-factor"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if let Some(v) = txt.and_then(|t| t.parse::<f64>().ok()) {
        return v.clamp(0.5, 3.0);
    }
    if let Ok(s) = std::env::var("GDK_SCALE") {
        if let Ok(v) = s.parse::<f64>() {
            if v >= 1.0 {
                return v;
            }
        }
    }
    1.0
}

#[tauri::command]
fn toggle_fullscreen(window: tauri::WebviewWindow) {
    let fs = window.is_fullscreen().unwrap_or(false);
    let _ = window.set_fullscreen(!fs);
}

fn read_settings(store: &Store) -> Initial {
    let f = |k: &str, d: f64| -> f64 {
        store
            .setting(k)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(d)
    };
    let eq: Vec<f64> = store
        .setting("eq")
        .and_then(|v| serde_json::from_str::<Vec<f64>>(&v).ok())
        .filter(|v| v.len() == 10)
        .unwrap_or_else(|| vec![0.0; 10]);
    let mut arr = [0.0f64; 10];
    arr.copy_from_slice(&eq);
    Initial {
        volume: f("volume", 1.0).clamp(0.0, 1.0),
        speed: f("speed", 1.0),
        pitch: f("pitch", 1.0),
        eq: arr,
    }
}

fn main() {
    log::init();
    let store = std::sync::Arc::new(Store::new());
    store.clone().spawn_saver();
    let opts: SharedOpts = std::sync::Arc::new(std::sync::Mutex::new(ThemeOpts::default()));
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .manage(opts.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let icon = app.default_window_icon().unwrap().clone();
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_icon(icon.clone());
            }
            let initial = read_settings(&store);
            let player = Player::new(handle.clone(), initial);
            let cb_app = handle.clone();
            {
                let mut cb = player.cb.lock().unwrap_or_else(|e| e.into_inner());
                *cb = Some(Box::new(move |e: Event| match e {
                    Event::Ended { tag, gapless } => {
                        let a = cb_app.clone();
                        tauri::async_runtime::spawn(async move { handle_ended(a, tag, gapless).await });
                    }
                    Event::Failed(msg) => {
                        log::line(&format!("playback failed: {msg}"));
                        let _ = cb_app.emit("playback-failed", msg);
                        if let Some(state) = cb_app.try_state::<AppState>() {
                            let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
                            if info.playing {
                                info.playing = false;
                                drop(info);
                                drop(state);
                                refresh_tray(&cb_app);
                            }
                        }
                    }
                    Event::Status { playing, position } => {
                        if let Some(state) = cb_app.try_state::<AppState>() {
                            *state.position.lock().unwrap_or_else(|e| e.into_inner()) = position;
                            let mut info = state.info.lock().unwrap_or_else(|e| e.into_inner());
                            if info.playing != playing {
                                info.playing = playing;
                                drop(info);
                                drop(state);
                                refresh_tray(&cb_app);
                            }
                        }
                    }
                }));
            }
            let shuffle = store.setting("shuffle").as_deref() == Some("1");
            let repeat = store
                .setting("repeat")
                .and_then(|r| match r.as_str() {
                    "all" => Some(Repeat::All),
                    "one" => Some(Repeat::One),
                    _ => Some(Repeat::Off),
                })
                .unwrap_or(Repeat::Off);
            let crossfade: u32 = store
                .setting("crossfade")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            player.send(Cmd::Fade(crossfade));
            app.manage(AppState {
                yt: Yt::new(),
                player,
                store: store.clone(),
                cache: Cache::new(),
                info: Mutex::new(PlayerInfo {
                    playing: false,
                    shuffle,
                    repeat: repeat.as_str().to_string(),
                    title: String::new(),
                    artist: String::new(),
                }),
                tray: Mutex::new(None),
                queue: Mutex::new(QueueState { shuffle, repeat, ..QueueState::default() }),
                position: Mutex::new(0.0),
                quality: Mutex::new(store.setting("quality").unwrap_or_else(|| "max".into())),
                tags: AtomicU64::new(1),
                preload_gen: AtomicU64::new(0),
                queue_gen: AtomicU64::new(0),
            });
            theming::watch(handle.clone(), opts.clone());
            #[cfg(target_os = "linux")]
            mpris::start(handle.clone());

            let info = app.state::<AppState>().info.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let menu = tray_menu(&handle, &info)?;
            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            state.store.save();
                        }
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    id => native_action(app, id.as_ref()),
                })
                .build(app)?;
            if let Some(state) = app.try_state::<AppState>() {
                *state.tray.lock().unwrap_or_else(|e| e.into_inner()) = Some(tray);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.store.save();
                }
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            search, home, playlists_for_you, album, album_more, artist, get_queue, play_tracks, play_radio, play_at,
            skip_next, skip_prev_cmd,
            toggle_play_cmd, pause, resume, seek, volume, toggle_shuffle_cmd, cycle_repeat_cmd,
            set_window_visible, set_quality, get_settings, set_speed, set_pitch, set_eq, set_crossfade, set_autocache, set_autoload,
            cache_info, cache_clear, add_to_queue, remove_from_queue, move_in_queue, clear_upcoming,
            likes_list, like_toggle, likes_clear, history_list, history_remove, history_clear,
            store_import, playlists_list, playlist_create, playlist_rename, playlist_delete,
            playlist_tracks, playlist_add, playlist_remove, lyrics, lyrics_get_custom, lyrics_save_custom,
            lyrics_delete_custom, lyrics_set_provider, lyrics_set_offset, set_panel_close, set_theme,
            system_scale, toggle_fullscreen
        ])
        .build(tauri::generate_context!())
        .expect("failed to build melissm")
        .run(|app, event| {
            use tauri::RunEvent;
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                if let Some(state) = app.try_state::<AppState>() {
                    state.store.save();
                }
            }
        });
}
