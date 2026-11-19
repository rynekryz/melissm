use gstreamer::{
    glib::types::Type,
    prelude::*,
    ClockTime, ElementFactory, SeekFlags, State,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

pub enum Cmd {
    Load { url: String, duration: f64, ua: String },
    Preload { url: String, ua: String, tag: u64 },
    PreloadAt { url: String, ua: String, tag: u64, position: f64 },
    CancelPreload,
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Volume(f32),
    Fade(u32),
    Mix { fade: f64, urgent: bool },
    CancelMix,
    MixTempo(f64),
    TempoReset,
    MixClock,
}

pub enum Event {
    Ended { tag: u64, gapless: bool },
    Failed(String),
    Status { playing: bool, position: f64 },
    Lull { position: f64, fade: f64, urgent: bool },
    Bpm(f64),
    Mixed { position: f64 },
    MixFailed,
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
    let level = ElementFactory::make("level").name("melissm_level").build()?;
    let bin = gstreamer::Bin::default();
    bin.add_many([&pitch, &eq, &level])?;
    pitch.link(&eq)?;
    eq.link(&level)?;
    level.set_property("post-messages", true);
    level.set_property("interval", ClockTime::from_mseconds(20));
    let sink = pitch
        .static_pad("sink")
        .ok_or_else(|| gstreamer::glib::bool_error!("pitch has no sink pad"))?;
    let src = level
        .static_pad("src")
        .ok_or_else(|| gstreamer::glib::bool_error!("level has no src pad"))?;
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
    speed: Arc<Mutex<f64>>,
}

impl Player {
    pub fn new(app: AppHandle, init: Initial) -> Self {
        gstreamer::init().ok();
        let (tx, rx) = mpsc::channel();
        let filters: Arc<Mutex<Vec<Filter>>> = Arc::new(Mutex::new(Vec::new()));
        let cb: Callback = Arc::new(Mutex::new(None));
        let speed: Arc<Mutex<f64>> = Arc::new(Mutex::new(init.speed));
        let f2 = filters.clone();
        let c2 = cb.clone();
        let s2 = speed.clone();
        thread::spawn(move || run(app, rx, f2, c2, s2, init));
        Self { tx, filters, cb, speed }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn set_speed(&self, value: f64) {
        *self.speed.lock().unwrap() = value;
        for f in self.filters.lock().unwrap().iter() {
            f.pitch.set_property("tempo", value.clamp(0.5, 2.0) as f32);
        }
        self.send(Cmd::TempoReset);
    }

    pub fn set_pitch(&self, value: f64) {
        for f in self.filters.lock().unwrap().iter() {
            f.pitch.set_property("pitch", value.clamp(0.25, 4.0) as f32);
        }
    }

    pub fn set_eq(&self, band: u32, gain: f64) {
        if band >= 10 {
            return;
        }
        for f in self.filters.lock().unwrap().iter() {
            f.eq.set_property(format!("band{band}").as_str(), gain.clamp(-24.0, 12.0));
        }
    }
}

const TEMPO_RATE: f64 = 50.0;

struct Tempo {
    hist: VecDeque<f64>,
    prev: f64,
}

impl Tempo {
    fn new() -> Self {
        Self { hist: VecDeque::with_capacity(1300), prev: f64::NEG_INFINITY }
    }

    fn reset(&mut self) {
        self.hist.clear();
        self.prev = f64::NEG_INFINITY;
    }

    fn push(&mut self, rms: f64) {
        if !rms.is_finite() {
            return;
        }
        let onset = if self.prev.is_finite() { (rms - self.prev).max(0.0) } else { 0.0 };
        self.prev = rms;
        if self.hist.len() >= 1200 {
            self.hist.pop_front();
        }
        self.hist.push_back(onset);
    }

    fn estimate(&self) -> Option<f64> {
        let n = self.hist.len();
        if n < 400 {
            return None;
        }
        let mean = self.hist.iter().sum::<f64>() / n as f64;
        let x: Vec<f64> = self.hist.iter().map(|v| v - mean).collect();
        let mut rs = [0.0f64; 51];
        for lag in 16..=50usize {
            if lag >= n {
                continue;
            }
            let m = n - lag;
            let mut num = 0.0;
            let mut ea = 0.0;
            let mut eb = 0.0;
            for i in 0..m {
                let a = x[i + lag];
                let b = x[i];
                num += a * b;
                ea += a * a;
                eb += b * b;
            }
            if ea > 0.0 && eb > 0.0 {
                rs[lag] = num / (ea.sqrt() * eb.sqrt());
            }
        }
        let mut best = 16usize;
        for lag in 16..=50usize {
            if rs[lag] > rs[best] {
                best = lag;
            }
        }
        let peak = rs[best];
        if peak < 0.1 {
            return None;
        }
        let mut rival = 0.0f64;
        for lag in 16..=50usize {
            if lag.abs_diff(best) > 4 && rs[lag] > rival {
                rival = rs[lag];
            }
        }
        if peak < rival * 1.15 {
            return None;
        }
        let mut period = best as f64;
        if best > 16 && best < 50 {
            let l = rs[best - 1];
            let r = rs[best + 1];
            let d = l - 2.0 * rs[best] + r;
            if d.abs() > 1e-9 {
                period += 0.5 * (l - r) / d;
            }
        }
        if period <= 0.0 {
            return None;
        }
        let mut bpm = TEMPO_RATE * 60.0 / period;
        while bpm < 80.0 {
            bpm *= 2.0;
        }
        while bpm > 160.0 {
            bpm /= 2.0;
        }
        bpm.is_finite().then_some(bpm)
    }
}

pub struct Analysis {
    pub bpm: Option<f64>,
    pub pick: Option<f64>,
}

pub fn analyze(url: &str, ua: &str, duration: f64) -> Analysis {
    use gstreamer::MessageView;
    let mut out = Analysis { bpm: None, pick: None };
    if gstreamer::init().is_err() {
        return out;
    }
    let playbin = match ElementFactory::make("playbin").build() {
        Ok(e) => e,
        Err(_) => return out,
    };
    let level = match ElementFactory::make("level").build() {
        Ok(e) => e,
        Err(_) => return out,
    };
    level.set_property("post-messages", true);
    level.set_property("interval", ClockTime::from_mseconds(20));
    playbin.set_property("audio-filter", &level);
    let sink = match ElementFactory::make("fakesink").build() {
        Ok(e) => e,
        Err(_) => return out,
    };
    sink.set_property("sync", false);
    playbin.set_property("audio-sink", &sink);
    let vsink = match ElementFactory::make("fakesink").build() {
        Ok(e) => e,
        Err(_) => return out,
    };
    vsink.set_property("sync", false);
    playbin.set_property("video-sink", &vsink);
    let ua = ua.to_string();
    playbin.connect("source-setup", false, move |values| {
        if let Ok(src) = values[1].get::<gstreamer::Element>() {
            if !ua.is_empty() && src.has_property("user-agent", Some(Type::STRING)) {
                src.set_property("user-agent", ua.as_str());
            }
        }
        None
    });
    playbin.set_property("uri", url);
    if playbin.set_state(State::Playing).is_err() {
        return out;
    }
    let Some(bus) = playbin.bus() else { return out };

    let rms_of = |msg: &gstreamer::Message| -> Option<(f64, f64)> {
        msg.structure().and_then(|st| {
            if st.name() == "level" {
                let rms = st.get::<gstreamer::glib::ValueArray>("rms").ok().map(|a| {
                    a.as_slice()
                        .iter()
                        .filter_map(|v| v.get::<f64>().ok())
                        .fold(f64::NEG_INFINITY, f64::max)
                })?;
                let at = st
                    .get::<ClockTime>("endtime")
                    .map(|v| v.seconds() as f64)
                    .unwrap_or(0.0);
                Some((rms, at))
            } else {
                None
            }
        })
    };

    let mut tempo = Tempo::new();
    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(15) {
            break;
        }
        let Some(msg) = bus.timed_pop(ClockTime::from_mseconds(100)) else { continue };
        if let Some((rms, _)) = rms_of(&msg) {
            tempo.push(rms);
            if tempo.hist.len() >= 1000 {
                break;
            }
        }
        if matches!(msg.view(), MessageView::Eos(..) | MessageView::Error(..)) {
            break;
        }
    }
    out.bpm = tempo.estimate();

    let target = if duration > 40.0 {
        (duration * 0.33).clamp(15.0, 100.0)
    } else {
        (duration * 0.4).max(12.0)
    };
    let from = (target - 2.0).max(0.0);
    let seek = playbin.seek_simple(
        SeekFlags::FLUSH | SeekFlags::KEY_UNIT,
        ClockTime::from_mseconds((from * 1000.0) as u64),
    );
    if seek.is_ok() {
        while bus.timed_pop(ClockTime::ZERO).is_some() {}
        let mut prev = f64::NEG_INFINITY;
        let mut best: Option<(f64, f64)> = None;
        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(8) {
                break;
            }
            let Some(msg) = bus.timed_pop(ClockTime::from_mseconds(100)) else { continue };
            if let Some((rms, at)) = rms_of(&msg) {
                if at > from + 0.4 && prev.is_finite() && rms > prev + 6.0 && rms > -30.0 {
                    let strength = rms - prev;
                    if best.is_none_or(|(s, _)| strength > s) {
                        best = Some((strength, at));
                    }
                }
                if at > from + 5.5 {
                    break;
                }
                prev = rms;
            }
            if matches!(msg.view(), MessageView::Eos(..) | MessageView::Error(..)) {
                break;
            }
        }
        out.pick = best.map(|(_, p)| (p - 0.02).max(0.0));
    }

    let _ = playbin.set_state(State::Null);
    out
}

fn run(
    app: AppHandle,
    rx: mpsc::Receiver<Cmd>,
    filters: Arc<Mutex<Vec<Filter>>>,
    cb: Callback,
    speed: Arc<Mutex<f64>>,
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
    let mut quiet = [None::<Instant>; 2];
    let mut mixing = false;
    let mut mix_secs = 0.0f64;
    let mut last_mix = Instant::now() - Duration::from_secs(600);
    let mut pending_seek = [None::<f64>; 2];
    let mut loud = [false; 2];
    let mut ema = [f64::NEG_INFINITY; 2];
    let mut dip = [None::<Instant>; 2];
    let mut mild = [None::<Instant>; 2];
    let mut tail_sent = [false; 2];
    let mut tempo = Tempo::new();
    let mut tempo_ratio = [1.0f64; 2];
    let mut bpm_last = 0.0f64;
    let mut bpm_tick = Instant::now();
    let mut beat_period = 0.0f64;
    let mut beat_ref = -1.0f64;
    let mut prev_rms = f64::NEG_INFINITY;
    let mut queued: Option<(f64, Instant)> = None;
    let mut mix_t0 = Instant::now();

    loop {
        let poll = queued
            .map(|(_, at)| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(60))
            .min(Duration::from_millis(60))
            .max(Duration::from_millis(4));
        match rx.recv_timeout(poll) {
            Ok(Cmd::Load { url, duration: dur, ua }) => {
                chains[0].stop();
                chains[1].stop();
                active = 0;
                chains[0].set_uri(&url, &ua, 0);
                chains[0].gain = 1.0;
                chains[0].apply_volume(volume);
                let _ = chains[0].playbin.set_state(State::Playing);
                duration = dur.max(0.0);
                fading = false;
                fade_pos = 0.0;
                mixing = false;
                quiet = [None, None];
                pending_seek = [None, None];
                loud = [false, false];
                ema = [f64::NEG_INFINITY; 2];
                dip = [None, None];
                mild = [None, None];
                tail_sent = [false, false];
                last_mix = Instant::now();
                tempo.reset();
                tempo_ratio = [1.0, 1.0];
                bpm_last = 0.0;
                beat_period = 0.0;
                beat_ref = -1.0;
                prev_rms = f64::NEG_INFINITY;
                queued = None;
                let user = *speed.lock().unwrap();
                chains[0].filter.pitch.set_property("tempo", user.clamp(0.5, 2.0) as f32);
                chains[1].filter.pitch.set_property("tempo", user.clamp(0.5, 2.0) as f32);
                chains[0].gain = 1.0;
                let _ = app.emit("player-state", Status { playing: true, position: 0.0, duration });
            }
            Ok(Cmd::Preload { url, ua, tag }) => {
                if !mixing {
                    let other = 1 - active;
                    chains[other].stop();
                    chains[other].set_uri(&url, &ua, tag);
                    chains[other].gain = 1.0;
                    chains[other].apply_volume(volume);
                    chains[other].filter.pitch.set_property("tempo", speed.lock().unwrap().clamp(0.5, 2.0) as f32);
                    tempo_ratio[other] = 1.0;
                    loud[other] = false;
                    pending_seek[other] = None;
                    let _ = chains[other].playbin.set_state(State::Paused);
                }
            }
            Ok(Cmd::PreloadAt { url, ua, tag, position }) => {
                if !mixing {
                    let other = 1 - active;
                    chains[other].stop();
                    chains[other].set_uri(&url, &ua, tag);
                    chains[other].gain = 1.0;
                    chains[other].apply_volume(volume);
                    chains[other].filter.pitch.set_property("tempo", speed.lock().unwrap().clamp(0.5, 2.0) as f32);
                    tempo_ratio[other] = 1.0;
                    loud[other] = false;
                    let _ = chains[other].playbin.set_state(State::Paused);
                    pending_seek[other] = if position > 0.5 { Some(position) } else { None };
                }
            }
            Ok(Cmd::CancelPreload) => {
                if !fading && !mixing {
                    chains[1 - active].stop();
                    pending_seek[1 - active] = None;
                }
            }
            Ok(Cmd::CancelMix) => {
                queued = None;
                if !fading && !mixing {
                    chains[1 - active].stop();
                    pending_seek[1 - active] = None;
                }
            }
            Ok(Cmd::Pause) => {
                queued = None;
                quiet[active] = None;
                let c = &mut chains[active];
                if c.playbin.current_state() >= State::Paused {
                    let _ = c.playbin.set_state(State::Paused);
                }
                if fading || mixing {
                    let o = &mut chains[1 - active];
                    if o.playbin.current_state() >= State::Paused {
                        let _ = o.playbin.set_state(State::Paused);
                    }
                }
            }
            Ok(Cmd::Resume) => {
                quiet[active] = None;
                let c = &mut chains[active];
                if c.ended {
                    let _ = c.playbin.seek_simple(SeekFlags::FLUSH | SeekFlags::KEY_UNIT, ClockTime::ZERO);
                    c.ended = false;
                }
                if c.playbin.current_state() == State::Paused {
                    let _ = c.playbin.set_state(State::Playing);
                }
                if fading || mixing {
                    let o = &chains[1 - active];
                    if o.playbin.current_state() == State::Paused {
                        let _ = o.playbin.set_state(State::Playing);
                    }
                }
            }
            Ok(Cmd::Stop) => {
                chains[0].stop();
                chains[1].stop();
                queued = None;
                beat_period = 0.0;
                beat_ref = -1.0;
                prev_rms = f64::NEG_INFINITY;
                fading = false;
                fade_pos = 0.0;
                mixing = false;
                quiet = [None, None];
                duration = 0.0;
                let _ = app.emit("player-state", Status::default());
            }
            Ok(Cmd::Seek(t)) => {
                beat_ref = -1.0;
                prev_rms = f64::NEG_INFINITY;
                quiet[active] = None;
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
            Ok(Cmd::MixClock) => {
                last_mix = Instant::now();
                quiet = [None, None];
                dip = [None, None];
                mild = [None, None];
                tail_sent = [false, false];
                tempo.reset();
                bpm_last = 0.0;
                beat_period = 0.0;
                beat_ref = -1.0;
                prev_rms = f64::NEG_INFINITY;
                queued = None;
            }
            Ok(Cmd::TempoReset) => {
                tempo_ratio = [1.0, 1.0];
            }
            Ok(Cmd::MixTempo(ratio)) => {
                let other = 1 - active;
                if chains[other].prepared {
                    let user = *speed.lock().unwrap();
                    let value = (user * ratio.clamp(0.85, 1.15)).clamp(0.5, 2.0);
                    chains[other].filter.pitch.set_property("tempo", value as f32);
                    tempo_ratio[other] = value / user.max(0.5);
                }
            }
            Ok(Cmd::Mix { fade, urgent }) => {
                let other = 1 - active;
                let playing = chains[active].playbin.current_state() == State::Playing;
                if !mixing && queued.is_none() && chains[other].prepared && playing {
                    let p = chains[active]
                        .playbin
                        .query_position::<ClockTime>()
                        .map(|v| v.seconds() as f64)
                        .unwrap_or(0.0);
                    let mut fade = fade;
                    if duration > 0.0 {
                        fade = fade.min((duration - p - 0.15).max(1.0));
                    }
                    let mut wait = 0.0f64;
                    if beat_period > 0.05 && beat_ref >= 0.0 {
                        let beats = (p - beat_ref) / beat_period;
                        let mut corner = (beats + 0.15).ceil();
                        let step = if urgent || beat_period * 8.0 > 4.0 { 4.0 } else { 8.0 };
                        let phase = corner.rem_euclid(step);
                        if phase > 0.0 {
                            corner += step - phase;
                        }
                        wait = ((beat_ref + corner * beat_period) - p).max(0.0);
                        if p + wait + fade > duration - 0.25 {
                            wait = 0.0;
                        }
                    }
                    let lead = if wait > 0.1 { 0.03 } else { 0.0 };
                    queued = Some((fade, Instant::now() + Duration::from_secs_f64((wait - lead).max(0.0))));
                } else if let Some(f) = cb.lock().unwrap().as_ref() {
                    f(Event::MixFailed);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        for ci in [0usize, 1usize] {
            if let Some(pos) = pending_seek[ci] {
                if chains[ci].prepared && chains[ci].playbin.current_state() >= State::Paused {
                    let ok = chains[ci].playbin.seek_simple(
                        SeekFlags::FLUSH | SeekFlags::KEY_UNIT,
                        ClockTime::from_mseconds((pos.max(0.0) * 1000.0) as u64),
                    );
                    if ok.is_ok() {
                        pending_seek[ci] = None;
                    }
                }
            }
        }

        if let Some((fade, at)) = queued {
            if Instant::now() >= at {
                let other = 1 - active;
                if pending_seek[other].is_none()
                    && chains[other].prepared
                    && chains[active].playbin.current_state() == State::Playing
                {
                    queued = None;
                    chains[other].gain = 0.0;
                    chains[other].apply_volume(volume);
                    let _ = chains[other].playbin.set_state(State::Playing);
                    mix_secs = fade.max(1.0);
                    mix_t0 = Instant::now();
                    mixing = true;
                    fading = false;
                    quiet = [None, None];
                    loud = [false, false];
                } else if Instant::now() >= at + Duration::from_millis(1500) {
                    queued = None;
                    if let Some(f) = cb.lock().unwrap().as_ref() {
                        f(Event::MixFailed);
                    }
                }
            }
        }

        let fade = fade_secs as f64;
        if fade > 0.0 && !fading && !mixing && queued.is_none() {
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
        if mixing {
            let mix_pos = mix_t0.elapsed().as_secs_f64();
            let k = (mix_pos / mix_secs).min(1.0);
            let smooth = |x: f64| {
                let x = x.clamp(0.0, 1.0);
                x * x * (3.0 - 2.0 * x)
            };
            let rise = (1.4 / mix_secs).clamp(0.12, 0.45);
            let attack = smooth(k / rise);
            let in_gain = 0.85 * attack + 0.15 * smooth((k - 0.4) / 0.6);
            let out_gain = 1.0 - smooth((k - 0.45) / 0.55);
            chains[active].gain = out_gain;
            chains[active].apply_volume(volume);
            let other = 1 - active;
            chains[other].gain = in_gain;
            chains[other].apply_volume(volume);
            if k >= 1.0 {
                let end_pos = chains[active]
                    .playbin
                    .query_position::<ClockTime>()
                    .map(|v| v.seconds() as f64)
                    .unwrap_or(0.0);
                let user = *speed.lock().unwrap();
                chains[active].filter.pitch.set_property("tempo", user.clamp(0.5, 2.0) as f32);
                tempo_ratio[active] = 1.0;
                chains[active].stop();
                chains[active].gain = 1.0;
                active = other;
                chains[active].gain = 1.0;
                chains[active].apply_volume(volume);
                mixing = false;
                last_mix = Instant::now();
                quiet = [None, None];
                ema = [f64::NEG_INFINITY; 2];
                dip = [None, None];
                mild = [None, None];
                tail_sent = [false, false];
                tempo.reset();
                bpm_last = 0.0;
                beat_period = 0.0;
                beat_ref = -1.0;
                prev_rms = f64::NEG_INFINITY;
                if let Some(f) = cb.lock().unwrap().as_ref() {
                    f(Event::Mixed { position: end_pos });
                }
            }
        }

        use gstreamer::MessageView;
        for ci in [active, 1 - active] {
            let c = &chains[ci];
            let Some(bus) = c.playbin.bus() else { continue };
            while let Some(msg) = bus.timed_pop(ClockTime::ZERO) {
                if ci == active && !fading && !mixing {
                    if let Some(st) = msg.structure() {
                        if st.name() == "level" {
                            let rms = st
                                .get::<gstreamer::glib::ValueArray>("rms")
                                .ok()
                                .map(|a| {
                                    a.as_slice()
                                        .iter()
                                        .filter_map(|v| v.get::<f64>().ok())
                                        .fold(f64::NEG_INFINITY, f64::max)
                                })
                                .unwrap_or(f64::NEG_INFINITY);
                            if rms.is_finite() {
                                ema[ci] = if ema[ci].is_finite() {
                                    ema[ci] * 0.92 + rms * 0.08
                                } else {
                                    rms
                                };
                            }
                            tempo.push(rms);
                            if beat_period > 0.05 {
                                let p_now = st
                                    .get::<ClockTime>("endtime")
                                    .map(|v| v.seconds() as f64)
                                    .unwrap_or_else(|_| {
                                        chains[ci]
                                            .playbin
                                            .query_position::<ClockTime>()
                                            .map(|v| v.seconds() as f64)
                                            .unwrap_or(0.0)
                                    });
                                if beat_ref < 0.0 && p_now > 1.0 {
                                    beat_ref = p_now;
                                } else if prev_rms.is_finite() && rms > prev_rms + 5.0 && rms > -30.0 {
                                    let beats = (p_now - beat_ref) / beat_period;
                                    let target = beat_ref + beats.round() * beat_period;
                                    let err = p_now - target;
                                    if err.abs() < 0.16 * beat_period {
                                        beat_ref += 0.35 * err;
                                    }
                                }
                            }
                            prev_rms = rms;
                            if rms > -18.0 {
                                quiet[ci] = None;
                                loud[ci] = true;
                            }
                            let now = Instant::now();
                            let avg = ema[ci];
                            if let Some(pos) = chains[ci].playbin.query_position::<ClockTime>() {
                                let p = pos.seconds() as f64;
                                let remain = duration - p;
                                let can = p >= 15.0 && remain >= 5.5 && loud[ci];
                                let mut fire: Option<f64> = None;
                                if can && rms < -28.0 {
                                    let since = *quiet[ci].get_or_insert(now);
                                    if now.duration_since(since) >= Duration::from_millis(700) {
                                        fire = Some(5.0);
                                    }
                                } else {
                                    quiet[ci] = None;
                                }
                                if can && avg > -24.0 && rms < avg - 6.0 {
                                    let since = *dip[ci].get_or_insert(now);
                                    if now.duration_since(since) >= Duration::from_millis(500) {
                                        fire = Some(5.0);
                                    }
                                } else {
                                    dip[ci] = None;
                                }
                                if fire.is_none()
                                    && loud[ci]
                                    && duration > 0.0
                                    && p >= 12.0
                                    && remain <= 6.0
                                    && remain >= 0.6
                                    && !tail_sent[ci]
                                {
                                    tail_sent[ci] = true;
                                    fire = Some(3.8);
                                }
                                if fire.is_none() {
                                    let gap = last_mix.elapsed();
                                    if gap >= Duration::from_secs(45) && p >= 25.0 && remain >= 8.0 {
                                        if gap >= Duration::from_secs(60) {
                                            fire = Some(6.0);
                                        } else if avg.is_finite() && rms < avg - 1.0 {
                                            let since = *mild[ci].get_or_insert(now);
                                            if now.duration_since(since) >= Duration::from_millis(300) {
                                                fire = Some(5.5);
                                            }
                                        } else {
                                            mild[ci] = None;
                                        }
                                    } else {
                                        mild[ci] = None;
                                    }
                                }
                                if let Some(fade) = fire {
                                    quiet[ci] = None;
                                    dip[ci] = None;
                                    mild[ci] = None;
                                    last_mix = now;
                                    let urgent = duration > 0.0 && (duration - p) < 9.0;
                                    if let Some(f) = cb.lock().unwrap().as_ref() {
                                        f(Event::Lull { position: p, fade, urgent });
                                    }
                                }
                            }
                            if bpm_tick.elapsed() >= Duration::from_secs(2) {
                                if let Some(bpm) = tempo.estimate() {
                                    let media = bpm / tempo_ratio[ci].max(0.5);
                                    if (media - bpm_last).abs() > 1.5 {
                                        bpm_last = media;
                                        beat_period = 60.0 / media;
                                        if let Some(f) = cb.lock().unwrap().as_ref() {
                                            f(Event::Bpm(media));
                                        }
                                    }
                                }
                                bpm_tick = Instant::now();
                            }
                        }
                    }
                }
                match msg.view() {
                    MessageView::Error(err) if ci == active || fading || mixing => {
                        chains[0].stop();
                        chains[1].stop();
                        let _ = app.emit("player-state", Status::default());
                        if let Some(f) = cb.lock().unwrap().as_ref() {
                            f(Event::Failed(err.error().to_string()));
                        }
                        active = 0;
                        fading = false;
                        fade_pos = 0.0;
                        mixing = false;
                        queued = None;
                        duration = 0.0;
                        quiet = [None, None];
                        pending_seek = [None, None];
                        loud = [false, false];
                        ema = [f64::NEG_INFINITY; 2];
                        dip = [None, None];
                        mild = [None, None];
                        tail_sent = [false, false];
                        tempo.reset();
                        tempo_ratio = [1.0, 1.0];
                        bpm_last = 0.0;
                        beat_period = 0.0;
                        beat_ref = -1.0;
                        prev_rms = f64::NEG_INFINITY;
                    }
                    MessageView::Error(..) => {
                        chains[ci].stop();
                    }
                    MessageView::Eos(..) if ci == active && !mixing => {
                        queued = None;
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
                            quiet = [None, None];
                            pending_seek = [None, None];
                            loud = [false, false];
                            ema = [f64::NEG_INFINITY; 2];
                            dip = [None, None];
                            mild = [None, None];
                            tail_sent = [false, false];
                            tempo.reset();
                            tempo_ratio = [1.0, 1.0];
                            bpm_last = 0.0;
                            beat_period = 0.0;
                            beat_ref = -1.0;
                            prev_rms = f64::NEG_INFINITY;
                            last_mix = Instant::now();
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
