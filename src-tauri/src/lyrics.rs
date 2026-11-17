use crate::store::Store;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const UA: &str = "Melissm/0.3.0 (https://github.com/rynekryz/melissm)";

#[derive(Serialize, Deserialize, Clone)]
pub struct Word {
    pub t: String,
    pub s: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KLine {
    pub text: String,
    pub start: f64,
    #[serde(default)]
    pub words: Option<Vec<Word>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub karaoke: Option<Vec<KLine>>,
    pub provider: String,
    pub custom: bool,
}

impl Lyrics {
    fn from_lrc(text: &str, provider: &str, custom: bool) -> Option<Lyrics> {
        let synced = Some(text.to_string());
        let lines = parse_lrc(text);
        if lines.is_empty() {
            let plain = (!text.trim().is_empty()).then(|| text.to_string());
            return plain.map(|plain| Lyrics { synced: None, plain: Some(plain), karaoke: None, provider: provider.into(), custom });
        }
        let karaoke = if lines.iter().all(|l| l.words.is_none()) { None } else { Some(lines) };
        Some(Lyrics { synced, plain: None, karaoke, provider: provider.into(), custom })
    }
}

fn parse_lrc(text: &str) -> Vec<KLine> {
    let mut out = vec![];
    for l in text.lines() {
        let Some(caps) = regex_lrc(l) else { continue };
        let (secs, rest) = caps;
        let mut line = KLine { text: rest.trim().to_string(), start: secs, words: None };
        if line.text.is_empty() {
            continue;
        }
        let mut words = vec![];
        for w in l.split_whitespace() {
            if let Some((t, word)) = word_time(w) {
                words.push(Word { t: format!("{word} "), s: t });
            }
        }
        if words.len() >= 2 {
            line.words = Some(words);
        }
        out.push(line);
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

fn regex_lrc(line: &str) -> Option<(f64, &str)> {
    let b = line.as_bytes();
    if b.first() != Some(&b'[') {
        return None;
    }
    let end = line.find(']')?;
    let ts = &line[1..end];
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    let min: f64 = parts[0].trim().parse().ok()?;
    let sec: f64 = parts[1].trim().parse().ok()?;
    let mut total = min * 60.0 + sec;
    if parts.len() == 3 {
        let frac = format!("0.{}", parts[2].trim());
        total += frac.parse::<f64>().unwrap_or(0.0);
    }
    Some((total, &line[end + 1..]))
}

fn word_time(w: &str) -> Option<(f64, &str)> {
    let s = w.strip_prefix('[')?;
    let end = s.find(']')?;
    let ts = &s[..end];
    let parts: Vec<&str> = ts.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let min: f64 = parts[0].trim().parse().ok()?;
    let sec: f64 = parts[1].trim().parse().ok()?;
    Some((min * 60.0 + sec, &s[end + 1..]))
}

#[derive(Deserialize)]
struct YFile {
    lines: Vec<YLine>,
}

#[derive(Deserialize)]
struct YLine {
    text: String,
    start_ms: f64,
    #[serde(default)]
    words: Option<Vec<YWord>>,
}

#[derive(Deserialize)]
struct YWord {
    text: String,
    start_ms: f64,
}

fn yaml_karaoke(yaml: &str) -> Option<Vec<KLine>> {
    let f: YFile = serde_yaml::from_str(yaml).ok()?;
    let lines: Vec<KLine> = f
        .lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| {
            let words = l.words.map(|ws| ws.into_iter().map(|w| Word { t: w.text, s: w.start_ms / 1000.0 }).collect());
            KLine { text: l.text, start: l.start_ms / 1000.0, words }
        })
        .collect();
    if lines.iter().all(|l| l.words.is_none()) {
        return None;
    }
    Some(lines)
}

fn lrclib_pick(v: &Value) -> Lyrics {
    let field = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    let synced = field("syncedLyrics");
    let plain = field("plainLyrics");
    let karaoke = v
        .get("lyricsfile")
        .and_then(Value::as_str)
        .and_then(yaml_karaoke);
    Lyrics {
        synced: if karaoke.is_some() { synced.clone() } else { synced },
        plain,
        karaoke,
        provider: "LRCLIB".into(),
        custom: false,
    }
}

async fn lrclib(http: &Client, title: &str, artist: &str, album: &str, duration: f64) -> Option<Lyrics> {
    let secs = duration.round().to_string();
    let exact = http
        .get("https://lrclib.net/api/get")
        .header("User-Agent", UA)
        .query(&[
            ("track_name", title),
            ("artist_name", artist),
            ("album_name", album),
            ("duration", secs.as_str()),
        ])
        .send()
        .await
        .ok()?;
    if exact.status().is_success() {
        let v: Value = exact.json().await.ok()?;
        let l = lrclib_pick(&v);
        if l.synced.is_some() || l.plain.is_some() {
            return Some(l);
        }
    }
    let found: Value = http
        .get("https://lrclib.net/api/search")
        .header("User-Agent", UA)
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    found
        .as_array()
        .and_then(|a| a.iter().map(lrclib_pick).find(|l| l.synced.is_some() || l.plain.is_some()))
}

async fn netease(http: &Client, title: &str, artist: &str) -> Option<Lyrics> {
    let v: Value = http
        .post("https://music.163.com/api/search/get")
        .header("Referer", "https://music.163.com")
        .form(&[
            ("s", format!("{title} {artist}")),
            ("type", "1".to_string()),
            ("offset", "0".to_string()),
            ("limit", "5".to_string()),
        ])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let id = v.pointer("/result/songs/0/id")?.as_i64()?.to_string();
    let v: Value = http
        .get("https://music.163.com/api/song/lyric")
        .header("Referer", "https://music.163.com")
        .query(&[("id", id.as_str()), ("lv", "-1"), ("kv", "-1"), ("tv", "-1")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let synced = v.pointer("/lrc/lyric").and_then(Value::as_str).filter(|s| !s.trim().is_empty())?;
    let l = Lyrics::from_lrc(synced, "NetEase", false)?;
    Some(l)
}

pub async fn fetch(store: &Store, http: &Client, id: &str, title: &str, artist: &str, album: &str, duration: f64) -> Lyrics {
    if let Some(custom) = store.custom_lyrics(id) {
        if let Some(l) = Lyrics::from_lrc(&custom, "You", true) {
            return l;
        }
    }
    let enabled = |k: &str| store.setting(k).as_deref() != Some("0");
    let mut result = Lyrics::default();
    if enabled("lyr_lrclib") {
        if let Some(l) = lrclib(http, title, artist, album, duration).await {
            if l.synced.is_some() || l.plain.is_some() {
                return l;
            }
            result = l;
        }
    }
    if enabled("lyr_netease") {
        if let Some(l) = netease(http, title, artist).await {
            return l;
        }
    }
    result
}
