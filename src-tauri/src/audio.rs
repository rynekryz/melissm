use gstreamer::{
    glib::types::Type,
    prelude::*,
    ClockTime, ElementFactory, SeekFlags, State,
};
use serde::Serialize;
use std::{
    sync::{
        atomic::AtomicU64,
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

pub enum Cmd {
    Load { url: String, duration: f64, ua: String },
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Volume(f32),
}

#[derive(Clone, Serialize, Default)]
struct Status {
    playing: bool,
    position: f64,
    duration: f64,
}

#[derive(Clone)]
pub struct Filter {
    pub chain: gstreamer::Element,
    pub pitch: gstreamer::Element,
    pub eq: gstreamer::Element,
}

fn build_filter() -> Result<Filter, gstreamer::glib::BoolError> {
    let pitch = ElementFactory::make("pitch").name("melissm_pitch").build()?;
    let eq = ElementFactory::make("equalizer-10bands").name("melissm_eq").build()?;
    let bin = gstreamer::Bin::default();
    bin.add_many([&pitch, &eq])?;
    pitch.link(&eq)?;
    let sink = pitch
        .static_pad("sink")
        .ok_or_else(|| gstreamer::glib::bool_error!("pitch has no sink pad"))?;
    let src = eq
        .static_pad("src")
        .ok_or_else(|| gstreamer::glib::bool_error!("equalizer has no src pad"))?;
    bin.add_pad(&gstreamer::GhostPad::with_target(&sink)?)?;
    bin.add_pad(&gstreamer::GhostPad::with_target(&src)?)?;
    Ok(Filter { chain: bin.upcast(), pitch, eq })
}

pub struct Player {
    tx: mpsc::Sender<Cmd>,
    pub generation: Arc<AtomicU64>,
    pub filter: Option<Filter>,
}

impl Player {
    pub fn new(app: AppHandle) -> Self {
        gstreamer::init().ok();
        let filter = build_filter().ok();
        let (tx, rx) = mpsc::channel();
        let f = filter.clone();
        thread::spawn(move || run(app, rx, f));
        Self {
            tx,
            generation: Arc::new(AtomicU64::new(0)),
            filter,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

fn run(app: AppHandle, rx: mpsc::Receiver<Cmd>, filter: Option<Filter>) {
    if gstreamer::init().is_err() {
        return;
    }
    let Ok(playbin) = ElementFactory::make("playbin").build() else {
        return;
    };
    if let Some(f) = &filter {
        playbin.set_property("audio-filter", &f.chain);
    }

    let pending_ua: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    {
        let pending_ua = pending_ua.clone();
        playbin.connect("source-setup", false, move |values| {
            if let Ok(src) = values[1].get::<gstreamer::Element>() {
                let ua = pending_ua.lock().map(|m| m.clone()).unwrap_or_default();
                if !ua.is_empty() && src.has_property("user-agent", Some(Type::STRING)) {
                    src.set_property("user-agent", ua);
                }
            }
            None
        });
    }

    let bus = playbin.bus().expect("playbin bus");
    let mut volume = 1.0f64;
    let mut duration = 0.0f64;
    let mut ended = false;

    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Cmd::Load { url, duration: dur, ua }) => {
                let _ = playbin.set_state(State::Null);
                duration = dur.max(0.0);
                ended = false;
                *pending_ua.lock().unwrap() = ua;
                playbin.set_property("uri", url);
                playbin.set_property("volume", volume);
                let _ = playbin.set_state(State::Playing);
            }
            Ok(Cmd::Pause) => {
                let _ = playbin.set_state(State::Paused);
            }
            Ok(Cmd::Resume) => {
                if ended {
                    let _ = playbin.seek_simple(
                        SeekFlags::FLUSH | SeekFlags::KEY_UNIT,
                        ClockTime::ZERO,
                    );
                    ended = false;
                    let _ = playbin.set_state(State::Playing);
                } else if playbin.current_state() == State::Paused {
                    let _ = playbin.set_state(State::Playing);
                }
            }
            Ok(Cmd::Stop) => {
                let _ = playbin.set_state(State::Null);
                let _ = app.emit("player-state", Status::default());
            }
            Ok(Cmd::Seek(t)) => {
                if playbin.current_state() >= State::Paused {
                    let _ = playbin.seek_simple(
                        SeekFlags::FLUSH | SeekFlags::KEY_UNIT,
                        ClockTime::from_mseconds((t.max(0.0) * 1000.0) as u64),
                    );
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = f64::from(v.clamp(0.0, 1.0));
                playbin.set_property("volume", volume);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        use gstreamer::MessageView;
        while let Some(msg) = bus.timed_pop(ClockTime::ZERO) {
            match msg.view() {
                MessageView::Eos(..) => {
                    if !ended {
                        ended = true;
                        let _ = app.emit("track-ended", ());
                    }
                }
                MessageView::Error(err) => {
                    let _ = playbin.set_state(State::Null);
                    let _ = app.emit("player-state", Status::default());
                    let _ = app.emit("playback-failed", err.error().to_string());
                    break;
                }
                _ => {}
            }
        }

        if playbin.current_state() >= State::Paused {
            let playing = playbin.current_state() == State::Playing && !ended;
            let position = playbin
                .query_position::<ClockTime>()
                .map(|p| p.seconds() as f64)
                .unwrap_or(if ended { duration } else { 0.0 });
            if let Some(d) = playbin.query_duration::<ClockTime>() {
                if d.seconds() as f64 > 0.0 {
                    duration = d.seconds() as f64;
                }
            }
            let _ = app.emit("player-state", Status { playing, position, duration });
            #[cfg(target_os = "linux")]
            crate::mpris::update_status(playing, position, duration);
        }
    }
}
