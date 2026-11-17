use gstreamer::{
    glib::types::Type,
    prelude::*,
    ClockTime, ElementFactory, SeekFlags, State,
};
use serde::Serialize;
use std::{
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

pub enum Cmd {
    Load { url: String, duration: f64, ua: String },
    Preload { url: String, ua: String, tag: u64 },
    CancelPreload,
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Volume(f32),
    Fade(u32),
}

pub enum Event {
    Ended { tag: u64, gapless: bool },
    Failed(String),
    Status { playing: bool, position: f64 },
}

pub type Callback = Arc<Mutex<Option<Box<dyn Fn(Event) + Send + Sync>>>>;

#[derive(Clone, Copy)]
pub struct Initial {
    pub volume: f64,
    pub speed: f64,
    pub pitch: f64,
    pub eq: [f64; 10],
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

struct Chain {
    playbin: gstreamer::Element,
    filter: Filter,
    ua: Arc<Mutex<String>>,
    tag: u64,
    prepared: bool,
    ended: bool,
    gain: f64,
}

impl Chain {
    fn build() -> Option<Self> {
        let filter = build_filter().ok()?;
        let playbin = ElementFactory::make("playbin").build().ok()?;
        playbin.set_property("audio-filter", &filter.chain);
        let ua: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let pending = ua.clone();
        playbin.connect("source-setup", false, move |values| {
            if let Ok(src) = values[1].get::<gstreamer::Element>() {
                let ua = pending.lock().map(|m| m.clone()).unwrap_or_default();
                if !ua.is_empty() && src.has_property("user-agent", Some(Type::STRING)) {
                    src.set_property("user-agent", ua);
                }
            }
            None
        });
        Some(Self { playbin, filter, ua, tag: 0, prepared: false, ended: false, gain: 1.0 })
    }

    fn set_uri(&mut self, url: &str, ua: &str, tag: u64) {
        let _ = self.playbin.set_state(State::Null);
        *self.ua.lock().unwrap() = ua.to_string();
        self.playbin.set_property("uri", url);
        self.tag = tag;
        self.prepared = true;
        self.ended = false;
    }

    fn stop(&mut self) {
        let _ = self.playbin.set_state(State::Null);
        self.prepared = false;
        self.ended = false;
    }

    fn apply_volume(&self, base: f64) {
        self.playbin.set_property("volume", base * self.gain);
    }
}

pub struct Player {
    tx: mpsc::Sender<Cmd>,
    pub filters: Arc<Mutex<Vec<Filter>>>,
    pub cb: Callback,
}

impl Player {
    pub fn new(app: AppHandle, init: Initial) -> Self {
        gstreamer::init().ok();
        let (tx, rx) = mpsc::channel();
        let filters: Arc<Mutex<Vec<Filter>>> = Arc::new(Mutex::new(Vec::new()));
        let cb: Callback = Arc::new(Mutex::new(None));
        let f2 = filters.clone();
        let c2 = cb.clone();
        thread::spawn(move || run(app, rx, f2, c2, init));
        Self { tx, filters, cb }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn set_speed(&self, value: f64) {
        for f in self.filters.lock().unwrap().iter() {
            f.pitch.set_property("tempo", value.clamp(0.5, 2.0) as f32);
        }
    }

    pub fn set_pitch(&self, value: f64) {
        for f in self.filters.lock().unwrap().iter() {
            f.pitch.set_property("pitch", value.clamp(0.25, 4.0) as f32);
        }
    }

    pub fn set_eq(&self, band: u32, gain: f64) {
        for f in self.filters.lock().unwrap().iter() {
            f.eq.set_property(format!("band{band}").as_str(), gain.clamp(-24.0, 12.0));
        }
    }
}

fn run(
    app: AppHandle,
    rx: mpsc::Receiver<Cmd>,
    filters: Arc<Mutex<Vec<Filter>>>,
    cb: Callback,
    init: Initial,
) {
    if gstreamer::init().is_err() {
        return;
    }
    let mut chains = match Chain::build().and_then(|a| Chain::build().map(|b| [a, b])) {
        Some(c) => c,
        None => return,
    };
    *filters.lock().unwrap() = chains.iter().map(|c| c.filter.clone()).collect();
    for f in filters.lock().unwrap().iter() {
        f.pitch.set_property("tempo", init.speed as f32);
        f.pitch.set_property("pitch", init.pitch as f32);
    }
    for band in 0..10 {
        for f in filters.lock().unwrap().iter() {
            f.eq.set_property(format!("band{band}").as_str(), init.eq[band].clamp(-24.0, 12.0));
        }
    }
    let mut active = 0usize;
    let mut volume = init.volume;
    let mut fade_secs = 0.0f64;
    let mut fading = false;
    let mut fade_pos = 0.0f64;
    let mut duration = 0.0f64;

    loop {
        match rx.recv_timeout(Duration::from_millis(60)) {
            Ok(Cmd::Load { url, duration: dur, ua }) => {
                chains[0].stop();
                chains[1].stop();
                active = 0;
                chains[0].set_uri(&url, &ua, 0);
                chains[0].apply_volume(volume);
                let _ = chains[0].playbin.set_state(State::Playing);
                duration = dur.max(0.0);
                fading = false;
                fade_pos = 0.0;
                chains[0].gain = 1.0;
                let _ = app.emit("player-state", Status { playing: true, position: 0.0, duration });
            }
            Ok(Cmd::Preload { url, ua, tag }) => {
                let other = 1 - active;
                chains[other].stop();
                chains[other].set_uri(&url, &ua, tag);
                chains[other].gain = 1.0;
                chains[other].apply_volume(volume);
                let _ = chains[other].playbin.set_state(State::Paused);
            }
            Ok(Cmd::CancelPreload) => {
                if !fading {
                    chains[1 - active].stop();
                }
            }
            Ok(Cmd::Pause) => {
                let c = &mut chains[active];
                if c.playbin.current_state() >= State::Paused {
                    let _ = c.playbin.set_state(State::Paused);
                }
                if fading {
                    let o = &mut chains[1 - active];
                    if o.playbin.current_state() >= State::Paused {
                        let _ = o.playbin.set_state(State::Paused);
                    }
                }
            }
            Ok(Cmd::Resume) => {
                let c = &mut chains[active];
                if c.ended {
                    let _ = c.playbin.seek_simple(SeekFlags::FLUSH | SeekFlags::KEY_UNIT, ClockTime::ZERO);
                    c.ended = false;
                }
                if c.playbin.current_state() == State::Paused {
                    let _ = c.playbin.set_state(State::Playing);
                }
                if fading {
                    let o = &chains[1 - active];
                    if o.playbin.current_state() == State::Paused {
                        let _ = o.playbin.set_state(State::Playing);
                    }
                }
            }
            Ok(Cmd::Stop) => {
                chains[0].stop();
                chains[1].stop();
                fading = false;
                fade_pos = 0.0;
                duration = 0.0;
                let _ = app.emit("player-state", Status::default());
            }
            Ok(Cmd::Seek(t)) => {
                let c = &mut chains[active];
                if c.playbin.current_state() >= State::Paused {
                    let _ = c.playbin.seek_simple(
                        SeekFlags::FLUSH | SeekFlags::KEY_UNIT,
                        ClockTime::from_mseconds((t.max(0.0) * 1000.0) as u64),
                    );
                    c.ended = false;
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = f64::from(v.clamp(0.0, 1.0));
                chains[0].apply_volume(volume);
                chains[1].apply_volume(volume);
            }
            Ok(Cmd::Fade(s)) => {
                fade_secs = f64::from(s);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let fade = fade_secs as f64;
        if fade > 0.0 && !fading {
            let a = &chains[active];
            let b = &chains[1 - active];
            if b.prepared && a.playbin.current_state() == State::Playing && !a.ended && duration > fade * 2.0 {
                if let Some(pos) = a.playbin.query_position::<ClockTime>() {
                    let pos = pos.seconds() as f64;
                    if pos >= 1.0 && pos >= duration - fade {
                        b.apply_volume(volume);
                        let _ = b.playbin.set_state(State::Playing);
                        fading = true;
                        fade_pos = 0.0;
                    }
                }
            }
        }
        if fading {
            fade_pos += 0.06;
            let k = (fade_pos / fade).min(1.0);
            chains[active].gain = 1.0 - k;
            chains[active].apply_volume(volume);
            chains[1 - active].gain = k;
            chains[1 - active].apply_volume(volume);
        }

        use gstreamer::MessageView;
        for ci in [active, 1 - active] {
            let c = &chains[ci];
            let Some(bus) = c.playbin.bus() else { continue };
            while let Some(msg) = bus.timed_pop(ClockTime::ZERO) {
                match msg.view() {
                    MessageView::Error(err) if ci == active || fading => {
                        chains[0].stop();
                        chains[1].stop();
                        let _ = app.emit("player-state", Status::default());
                        if let Some(f) = cb.lock().unwrap().as_ref() {
                            f(Event::Failed(err.error().to_string()));
                        }
                        return;
                    }
                    MessageView::Error(..) => {
                        chains[ci].stop();
                    }
                    MessageView::Eos(..) if ci == active => {
                        let other = 1 - active;
                        if chains[other].prepared {
                            let tag = chains[other].tag;
                            if !fading {
                                chains[other].apply_volume(volume);
                                let _ = chains[other].playbin.set_state(State::Playing);
                            }
                            chains[active].stop();
                            active = other;
                            fading = false;
                            fade_pos = 0.0;
                            chains[active].gain = 1.0;
                            if let Some(f) = cb.lock().unwrap().as_ref() {
                                f(Event::Ended { tag, gapless: true });
                            }
                        } else {
                            chains[active].ended = true;
                            if let Some(f) = cb.lock().unwrap().as_ref() {
                                f(Event::Ended { tag: 0, gapless: false });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let a = &chains[active];
        if a.playbin.current_state() >= State::Paused {
            let playing = a.playbin.current_state() == State::Playing && !a.ended;
            let position = a
                .playbin
                .query_position::<ClockTime>()
                .map(|p| p.seconds() as f64)
                .unwrap_or(if a.ended { duration } else { 0.0 });
            if let Some(d) = a.playbin.query_duration::<ClockTime>() {
                if d.seconds() as f64 > 0.0 {
                    duration = d.seconds() as f64;
                }
            }
            let _ = app.emit("player-state", Status { playing, position, duration });
            #[cfg(target_os = "linux")]
            crate::mpris::update_status(playing, position, duration);
            if let Some(f) = cb.lock().unwrap().as_ref() {
                f(Event::Status { playing, position });
            }
        }
    }
}
