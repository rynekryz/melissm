use reqwest::{header, Client};
use serde::Serialize;
use serde_json::{json, Value};

const BASE: &str = "https://music.youtube.com/youtubei/v1";
const WEB_UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const IOS_UA: &str = "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";

#[derive(Serialize, Clone, Default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(rename = "album")]
    pub album: String,
    pub duration: f64,
    pub thumbnail: String,
    #[serde(default)]
    pub is_album: bool,
}

#[derive(Serialize, Clone)]
pub struct Section {
    pub title: String,
    pub tracks: Vec<Track>,
}

#[derive(Clone)]
pub struct Yt {
    pub http: Client,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn web_context() -> Value {
    json!({ "client": { "clientName": "WEB_REMIX", "clientVersion": "1.20251020.03.00", "hl": "en", "gl": "US" } })
}

pub fn collect<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                } else {
                    collect(x, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect(x, key, out)),
        _ => {}
    }
}

pub fn runs_groups(runs: Option<&Value>) -> Vec<String> {
    let mut groups = vec![];
    let mut cur = String::new();
    for r in runs.and_then(Value::as_array).into_iter().flatten() {
        let t = r.get("text").and_then(Value::as_str).unwrap_or("");
        if t.trim() == "•" {
            groups.push(std::mem::take(&mut cur));
        } else {
            cur.push_str(t);
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

fn thumbnail(r: &Value, path: &str) -> String {
    let url = r
        .pointer(path)
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("");
    match url.rfind("=w") {
        Some(i) if url.contains("googleusercontent") => format!("{}=w544-h544-l90-rj", &url[..i]),
        _ => url.to_string(),
    }
}

fn parse_seconds(s: &str) -> f64 {
    s.split(':')
        .try_fold(0.0, |acc, p| p.trim().parse::<f64>().ok().map(|n| acc * 60.0 + n))
        .unwrap_or(0.0)
}

fn parse_list_item(r: &Value) -> Option<Track> {
    let id = r
        .pointer("/playlistItemData/videoId")
        .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str)?;
    let cols = r.get("flexColumns")?.as_array()?;
    let runs = |i: usize| cols.get(i)?.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs");
    let title = runs(0)?.get(0)?.get("text")?.as_str()?.to_string();
    let meta = runs_groups(runs(1));
    let duration = meta
        .last()
        .filter(|g| g.contains(':'))
        .map(|g| parse_seconds(g))
        .unwrap_or(0.0);
    Some(Track {
        id: id.into(),
        title,
        artist: meta.first().cloned().unwrap_or_default(),
        album: if meta.len() >= 3 { meta[1].clone() } else { String::new() },
        duration,
        thumbnail: thumbnail(r, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"),
        is_album: false,
    })
}

pub fn parse_two_row(r: &Value) -> Option<Track> {
    let title = r.pointer("/title/runs/0/text")?.as_str()?.to_string();
    let groups = runs_groups(r.pointer("/subtitle/runs"));
    let kind = groups.first().cloned().unwrap_or_default();
    let artist = groups
        .iter()
        .find(|g| !matches!(g.as_str(), "Song" | "Video" | "Single" | "Album" | "EP" | "Artist" | "Playlist"))
        .cloned()
        .unwrap_or_default();
    if let Some(id) = r.pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(Value::as_str) {
        return Some(Track {
            id: id.into(),
            title,
            artist,
            album: String::new(),
            duration: 0.0,
            thumbnail: thumbnail(r, "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails"),
            is_album: false,
        });
    }
    let browse = r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str)?;
    let is_album = browse.starts_with("MPREb");
    Some(Track {
        id: browse.into(),
        title,
        artist,
        album: kind,
        duration: 0.0,
        thumbnail: thumbnail(r, "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails"),
        is_album,
    })
}

fn dedupe(mut tracks: Vec<Track>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    tracks.retain(|t| seen.insert(t.id.clone()));
    tracks
}

fn extract_visitor(page: &str) -> String {
    const N: &str = "\"visitorData\":\"";
    let Some(i) = page.find(N) else { return String::new() };
    let rest = &page[i + N.len()..];
    let end = rest.find('"').unwrap_or(0);
    rest[..end].replace("\\u003d", "=").replace("\\u0026", "&")
}

impl Yt {
    pub fn new() -> Self {
        Self { http: Client::builder().cookie_store(true).build().expect("http client") }
    }

    pub async fn post(&self, endpoint: &str, body: Value, ua: &str, web: bool) -> Result<Value, String> {
        let mut req = self
            .http
            .post(format!("{BASE}/{endpoint}?prettyPrint=false"))
            .header(header::USER_AGENT, ua)
            .json(&body);
        if web {
            req = req
                .header("Origin", "https://music.youtube.com")
                .header("Referer", "https://music.youtube.com/");
        }
        req.send().await.map_err(err)?.error_for_status().map_err(err)?.json().await.map_err(err)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        let body = json!({ "context": web_context(), "query": query, "params": SONGS_FILTER });
        let v = self.post("search", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "musicResponsiveListItemRenderer", &mut items);
        Ok(dedupe(items.into_iter().filter_map(parse_list_item).collect()))
    }

    pub async fn home(&self) -> Result<Vec<Section>, String> {
        let body = json!({ "context": web_context(), "browseId": "FEmusic_home" });
        let v = self.post("browse", body, WEB_UA, true).await?;
        let mut carousels = vec![];
        collect(&v, "musicCarouselShelfRenderer", &mut carousels);
        let mut sections = Vec::new();
        for c in &carousels {
            let title = c
                .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let mut songs = vec![];
            collect(c, "musicResponsiveListItemRenderer", &mut songs);
            let mut cards = vec![];
            collect(c, "musicTwoRowItemRenderer", &mut cards);
            let mut tracks: Vec<Track> = songs.into_iter().filter_map(parse_list_item).collect();
            tracks.extend(cards.into_iter().filter_map(parse_two_row));
            let tracks = dedupe(tracks);
            if !tracks.is_empty() {
                sections.push(Section { title: title.to_string(), tracks });
            }
        }
        if sections.is_empty() {
            return Ok(vec![Section { title: "Trending now".into(), tracks: self.search("top hits").await? }]);
        }
        Ok(sections)
    }

    pub async fn album(&self, id: &str) -> Result<Vec<Track>, String> {
        let body = json!({ "context": web_context(), "browseId": id });
        let v = self.post("browse", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "musicResponsiveListItemRenderer", &mut items);
        let tracks = dedupe(items.into_iter().filter_map(parse_list_item).collect());
        if tracks.is_empty() {
            return Err("empty album".into());
        }
        Ok(tracks)
    }
    async fn watch_page(&self, id: &str) -> Result<String, String> {
        let page = self
            .http
            .get(format!("https://music.youtube.com/watch?v={id}"))
            .header(header::USER_AGENT, VISIONOS_UA)
            .send()
            .await
            .map_err(err)?
            .error_for_status()
            .map_err(err)?
            .text()
            .await
            .map_err(err)?;
        Ok(page)
    }

    fn visionos_body(id: &str, visitor: &str) -> Value {
        json!({
            "context": { "client": {
                "clientName": "VISIONOS", "clientVersion": "1.02",
                "deviceMake": "Apple", "deviceModel": "RealityDevice17,1",
                "userAgent": VISIONOS_UA, "osName": "visionOS", "osVersion": "26.5.23O471",
                "visitorData": visitor, "hl": "en", "gl": "US"
            }},
            "videoId": id, "contentCheckOk": true, "racyCheckOk": true
        })
    }

    fn stream_attempts(id: &str, visitor: &str) -> Vec<(Value, &'static str, bool)> {
        vec![
            (Self::visionos_body(id, visitor), VISIONOS_UA, false),
            (
                json!({
                    "context": { "client": {
                        "clientName": "IOS", "clientVersion": "20.10.4",
                        "deviceMake": "Apple", "deviceModel": "iPhone16,2",
                        "osName": "iPhone", "osVersion": "18.3.2.22D82", "hl": "en", "gl": "US"
                    }},
                    "videoId": id, "contentCheckOk": true, "racyCheckOk": true
                }),
                IOS_UA,
                false,
            ),
        ]
    }

    pub async fn resolve_stream(&self, id: &str, quality: &str) -> Result<(String, f64, &'static str), String> {
        let visitor = extract_visitor(&self.watch_page(id).await.unwrap_or_default());

        for (body, ua, web) in Self::stream_attempts(id, &visitor) {
            let Ok(v) = self.post("player", body, ua, web).await else { continue };
            if v.pointer("/playabilityStatus/status").and_then(Value::as_str) != Some("OK") {
                continue;
            }
            let duration = v
                .pointer("/videoDetails/lengthSeconds")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let formats = v.pointer("/streamingData/adaptiveFormats").and_then(Value::as_array);
            let audio: Vec<&Value> = formats
                .map(|a| {
                    a.iter()
                        .filter(|f| f["mimeType"].as_str().is_some_and(|m| m.starts_with("audio/")) && f["url"].is_string())
                        .collect()
                })
                .unwrap_or_default();
            let pick = match quality {
                "low" => audio.iter().copied().min_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)),
                "high" => audio
                    .iter()
                    .copied()
                    .filter(|f| f["bitrate"].as_u64().unwrap_or(0) <= 192_000)
                    .max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)),
                _ => audio.iter().copied().max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)),
            };
            let pick = pick.or_else(|| audio.iter().copied().max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)));
            if let Some(f) = pick.and_then(|f| f["url"].as_str()) {
                return Ok((f.to_string(), duration, ua));
            }
        }
        Err("no playable stream".into())
    }
}

fn parse_radio_item(r: &Value) -> Option<Track> {
    let id = r.get("videoId").and_then(Value::as_str)?;
    let title = r.pointer("/title/runs/0/text")?.as_str()?.to_string();
    let artist = r.pointer("/shortBylineText/runs/0/text").and_then(Value::as_str).unwrap_or_default().to_string();
    let duration = r
        .pointer("/lengthText/runs/0/text")
        .and_then(Value::as_str)
        .map(parse_seconds)
        .unwrap_or(0.0);
    let thumb = r
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|t| t.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(Track {
        id: id.into(),
        title,
        artist,
        album: String::new(),
        duration,
        thumbnail: thumb,
        is_album: false,
    })
}

impl Yt {
    pub async fn radio(&self, id: &str) -> Result<Vec<Track>, String> {
        let body = json!({
            "context": web_context(),
            "videoId": id,
            "playlistId": format!("RDAMVM{id}"),
        });
        let v = self.post("next", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "playlistPanelVideoRenderer", &mut items);
        Ok(dedupe(
            items
                .into_iter()
                .filter_map(parse_radio_item)
                .filter(|t| t.id != id)
                .collect(),
        ))
    }
}
