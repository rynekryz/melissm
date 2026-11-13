#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod innertube;
mod lyrics;
mod theming;

use audio::{Cmd, Player};
use innertube::{Track, Yt};
use lyrics::Lyrics;
use serde::Serialize;
use std::{collections::BTreeMap, sync::atomic::Ordering, sync::Mutex};
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem},
    tray::TrayIcon,
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State, WindowEvent, Wry,
};
use theming::{SharedOpts, ThemeOpts};

struct AppState {
    yt: Yt,
    player: Player,
    info: Mutex<PlayerInfo>,
    tray: Mutex<Option<TrayIcon>>,
    handle: AppHandle,
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

#[tauri::command]
async fn search(state: State<'_, AppState>, query: String) -> Result<Vec<Track>, String> {
    state.yt.search(&query).await
}

#[tauri::command]
async fn home(state: State<'_, AppState>) -> Result<Vec<innertube::Section>, String> {
    state.yt.home().await
}

#[tauri::command]
async fn album(state: State<'_, AppState>, id: String) -> Result<Vec<Track>, String> {
    state.yt.album(&id).await
}

#[tauri::command]
async fn play(state: State<'_, AppState>, id: String) -> Result<f64, String> {
    let generation = state.player.generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.player.send(Cmd::Stop);
    let (url, duration, ua) = state.yt.resolve_stream(&id).await?;
    if state.player.generation.load(Ordering::SeqCst) != generation {
        return Err("superseded".into());
    }
    state.player.send(Cmd::Load { url, duration, ua: ua.to_string() });
    Ok(duration)
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
    state.player.send(Cmd::Volume(value));
}

#[tauri::command]
async fn lyrics(
    state: State<'_, AppState>,
    title: String,
    artist: String,
    album: String,
    duration: f64,
) -> Result<Lyrics, String> {
    lyrics::fetch(&state.yt.http, &title, &artist, &album, duration).await
}

#[tauri::command]
fn sync_player_state(
    state: State<'_, AppState>,
    playing: bool,
    shuffle: bool,
    repeat: String,
    title: String,
    artist: String,
) {
    *state.info.lock().unwrap() = PlayerInfo { playing, shuffle, repeat, title, artist };
    let info = state.info.lock().unwrap().clone();
    if let Some(tray) = state.tray.lock().unwrap().as_mut() {
        if let Ok(menu) = tray_menu(&state.handle, &info) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

#[tauri::command]
async fn set_theme(opts: State<'_, SharedOpts>, theme: ThemeOpts) -> Result<BTreeMap<String, String>, String> {
    *opts.lock().unwrap() = theme;
    let snapshot = opts.lock().unwrap().clone();
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

fn main() {
    let opts: SharedOpts = std::sync::Arc::new(std::sync::Mutex::new(ThemeOpts::default()));
    tauri::Builder::default()
        .manage(opts.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let icon = app.default_window_icon().unwrap().clone();
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_icon(icon.clone());
            }
            app.manage(AppState {
                yt: Yt::new(),
                player: Player::new(handle.clone()),
                info: Mutex::new(PlayerInfo::default()),
                tray: Mutex::new(None),
                handle: handle.clone(),
            });
            theming::watch(handle.clone(), opts.clone());

            let menu = tray_menu(&handle, &PlayerInfo::default())?;
            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    id => {
                        let _ = app.emit("tray-action", id.to_string());
                    }
                })
                .build(app)?;
            if let Some(state) = app.try_state::<AppState>() {
                *state.tray.lock().unwrap() = Some(tray);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            search, home, album, play, pause, resume, seek, volume, lyrics, set_theme,
            system_scale, sync_player_state
        ])
        .run(tauri::generate_context!())
        .expect("failed to run melissm");
}
