use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const BASE: &str = "https://music.youtube.com/youtubei/v1";
const WEB_UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const IOS_UA: &str = "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";
const SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
const PLAYLISTS_FILTER: &str = "EgeKAQQoAEABagoQBRAKEAMQBA==";
const SEARCH_ALL: &str = "EhGKAQ4IARABGAEgASgAOAFAAUICCAE=";
const SEARCH_SONGS: &str = "EgWKAQIIAUICCAFqDBAOEAoQAxAEEAkQBQ==";
const SEARCH_VIDEOS: &str = "EgWKAQIQAUICCAFqDBAOEAoQAxAEEAkQBQ==";
const SEARCH_ALBUMS: &str = "EgWKAQIYAUICCAFqDBAOEAoQAxAEEAkQBQ==";
const SEARCH_ARTISTS: &str = "EgWKAQIgAUICCAFqDBAOEAoQAxAEEAkQBQ==";
const SEARCH_PROFILES: &str = "EgWKAQJYAUICCAFqDBAOEAoQAxAEEAkQBQ==";
const SEARCH_COMMUNITY: &str = "EgeKAQQoAEABQgIIAWoMEA4QChADEAQQCRAF";
const SEARCH_PLAYLISTS: &str = "EgeKAQQoADgBQgIIAWoMEA4QChADEAQQCRAF";

#[derive(Serialize, Deserialize, Clone, Default)]
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
    #[serde(default)]
    pub is_artist: bool,
    #[serde(default)]
    pub is_playlist: bool,
    #[serde(default)]
    pub is_video: bool,
    #[serde(default)]
    pub is_profile: bool,
    #[serde(default)]
    pub artist_id: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Section {
    pub title: String,
    pub tracks: Vec<Track>,
}

#[derive(Serialize, Clone)]
pub struct SearchSection {
    pub title: String,
    pub kind: String,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Song,
    Video,
    Album,
    Artist,
    Playlist,
    Profile,
}

impl RowKind {
    fn title(self) -> &'static str {
        match self {
            Self::Song => "Songs",
            Self::Video => "Videos",
            Self::Album => "Albums",
            Self::Artist => "Artists",
            Self::Playlist => "Playlists",
            Self::Profile => "Profiles",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Song => "songs",
            Self::Video => "videos",
            Self::Album => "albums",
            Self::Artist => "artists",
            Self::Playlist => "playlists",
            Self::Profile => "profiles",
        }
    }
}

#[derive(Serialize, Clone)]
pub struct AlbumPage {
    pub title: String,
    pub subtitle: String,
    pub thumbnail: String,
    pub tracks: Vec<Track>,
    pub next: Option<String>,
}

pub fn playlist_continuation(v: &Value) -> Option<String> {
    let mut tokens = vec![];
    collect(v, "continuationCommand", &mut tokens);
    tokens
        .iter()
        .filter_map(|t| t.get("token").and_then(Value::as_str))
        .filter(|s| s.len() > 32)
        .next()
        .map(|s| s.to_string())
}

fn parse_album(v: &Value, id: &str) -> Result<AlbumPage, String> {
    let mut items = vec![];
    collect(v, "musicResponsiveListItemRenderer", &mut items);
    let mut tracks = dedupe(items.into_iter().filter_map(parse_list_item).collect());
    if tracks.is_empty() {
        return Err("empty album".into());
    }
    let mut headers = vec![];
    collect(v, "musicDetailHeaderRenderer", &mut headers);
    collect(v, "musicResponsiveHeaderRenderer", &mut headers);
    let header = headers.first();
    let title = header
        .and_then(|h| h.pointer("/title/runs/0/text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let subtitle = header
        .and_then(|h| h.pointer("/subtitle/runs"))
        .and_then(Value::as_array)
        .map(|rs| rs.iter().filter_map(|r| r.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let thumb = header
        .map(|h| thumbnail(h, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"))
        .filter(|s| !s.is_empty())
        .or_else(|| tracks.iter().map(|t| t.thumbnail.clone()).find(|s| !s.is_empty()))
        .unwrap_or_default();
    if !thumb.is_empty() {
        for t in &mut tracks {
            if t.thumbnail.is_empty() {
                t.thumbnail = thumb.clone();
            }
        }
    }
    let next = if id.starts_with("VL") || id.starts_with("PL") { playlist_continuation(v) } else { None };
    Ok(AlbumPage { title, subtitle, thumbnail: thumb, tracks, next })
}

#[derive(Serialize, Clone)]
pub struct ArtistPage {
    pub name: String,
    pub thumbnail: String,
    pub top: Vec<Track>,
    pub albums: Vec<Track>,
    pub singles: Vec<Track>,
    pub is_profile: bool,
}

struct Stream {
    url: String,
    duration: f64,
    ua: &'static str,
    at: Instant,
}

pub struct Yt {
    pub http: Client,
    streams: Mutex<HashMap<String, Stream>>,
    visitor: Mutex<Option<(String, Instant)>>,
}

const STREAM_TTL: Duration = Duration::from_secs(30 * 60);
const VISITOR_TTL: Duration = Duration::from_secs(60 * 60);

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

fn row_title(r: &Value) -> Option<String> {
    let runs = r
        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs")?
        .as_array()?;
    Some(runs.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect())
}

fn row_groups(r: &Value) -> Vec<String> {
    runs_groups(r.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs"))
}

fn row_browse(r: &Value) -> Option<&str> {
    r.pointer("/navigationEndpoint/browseEndpoint/browseId")
        .or_else(|| r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/browseEndpoint/browseId"))
        .and_then(Value::as_str)
}

fn row_video_id(r: &Value) -> Option<&str> {
    r.pointer("/playlistItemData/videoId")
        .or_else(|| r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .or_else(|| r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str)
}

fn row_video_type(r: &Value) -> &str {
    r.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
        .or_else(|| r.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn row_artist_id(r: &Value) -> Option<String> {
    r.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(Value::as_array)
        .and_then(|rs| {
            rs.iter().find_map(|run| {
                run.pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .filter(|id| id.starts_with("UC") || (id.starts_with("MP") && !id.starts_with("MPREb")))
                    .map(str::to_string)
            })
        })
}

fn row_thumbnail(r: &Value) -> String {
    thumbnail(r, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
}

fn row_duration(r: &Value, groups: &[String]) -> f64 {
    groups
        .iter()
        .rev()
        .find(|g| g.contains(':'))
        .map(|g| parse_seconds(g))
        .or_else(|| {
            r.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
                .and_then(Value::as_str)
                .filter(|s| s.contains(':'))
                .map(parse_seconds)
        })
        .unwrap_or(0.0)
}

fn is_song_video(vtype: &str) -> bool {
    matches!(
        vtype,
        "MUSIC_VIDEO_TYPE_ATV"
            | "MUSIC_VIDEO_TYPE_OMV"
            | "MUSIC_VIDEO_TYPE_UGC"
            | "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC"
    )
}

fn without_kind(groups: &[String]) -> &[String] {
    if matches!(groups.first().map(String::as_str), Some("Song" | "Video")) {
        &groups[1..]
    } else {
        groups
    }
}

fn parse_list_item(r: &Value) -> Option<Track> {
    let id = row_video_id(r)?;
    let title = row_title(r)?;
    let groups = row_groups(r);
    let rest = without_kind(&groups);
    let vtype = row_video_type(r);
    let is_video = vtype != "MUSIC_VIDEO_TYPE_ATV" && is_song_video(vtype);
    let artist = rest.first().cloned().unwrap_or_default();
    let album = if !is_video && rest.len() >= 3 { rest[1].clone() } else { String::new() };
    Some(Track {
        id: id.into(),
        title,
        artist,
        album,
        duration: row_duration(r, rest),
        thumbnail: row_thumbnail(r),
        is_album: false,
        is_artist: false,
        is_playlist: false,
        is_video,
        is_profile: false,
        artist_id: row_artist_id(r),
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
    let artist_id = r
        .pointer("/subtitle/runs")
        .and_then(Value::as_array)
        .and_then(|rs| {
            rs.iter().find_map(|run| {
                run.pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                    .filter(|id| id.starts_with("UC") || (id.starts_with("MP") && !id.starts_with("MPREb")))
                    .map(str::to_string)
            })
        });
    let thumbnail = thumbnail(r, "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails");
    if let Some(browse) = r.pointer("/navigationEndpoint/browseEndpoint/browseId").and_then(Value::as_str) {
        let is_album = browse.starts_with("MPREb");
        let is_playlist = browse.starts_with("VL") || browse.starts_with("PL");
        let is_artist = browse.starts_with("MP") && !is_album;
        return Some(Track {
            id: browse.into(),
            title,
            artist,
            album: kind,
            duration: 0.0,
            thumbnail,
            is_album,
            is_artist,
            is_playlist,
            is_video: false,
            is_profile: false,
            artist_id,
        });
    }
    let id = r.pointer("/navigationEndpoint/watchEndpoint/videoId").and_then(Value::as_str)?;
    let vtype = r
        .pointer("/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !is_song_video(vtype) {
        return None;
    }
    Some(Track {
        id: id.into(),
        title,
        artist,
        album: String::new(),
        duration: 0.0,
        thumbnail,
        is_album: false,
        is_artist: false,
        is_playlist: false,
        is_video: vtype != "MUSIC_VIDEO_TYPE_ATV",
        is_profile: false,
        artist_id,
    })
}

fn parse_playlist_row(r: &Value) -> Option<Track> {
    match parse_search_row(r) {
        Some((RowKind::Playlist, track)) => Some(track),
        _ => None,
    }
}

fn dedupe(mut tracks: Vec<Track>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    tracks.retain(|t| seen.insert(t.id.clone()));
    tracks
}

fn parse_search_row(r: &Value) -> Option<(RowKind, Track)> {
    let title = row_title(r)?;
    let groups = row_groups(r);
    let thumbnail = row_thumbnail(r);
    let artist_id = row_artist_id(r);
    let duration = row_duration(r, &groups);
    let (kind, id, artist, album, is_album, is_artist, is_playlist, is_video, is_profile) =
        if let Some(browse) = row_browse(r) {
            if browse.starts_with("MPREb") {
                (
                    RowKind::Album,
                    browse.to_string(),
                    groups.get(1).cloned().unwrap_or_default(),
                    groups.get(2).cloned().unwrap_or_default(),
                    true,
                    false,
                    false,
                    false,
                    false,
                )
            } else if browse.starts_with("VL") || browse.starts_with("PL") {
                let author = if groups.first().map(String::as_str) == Some("Playlist") {
                    groups.get(1)
                } else {
                    groups.first()
                };
                (
                    RowKind::Playlist,
                    browse.to_string(),
                    author.cloned().unwrap_or_default(),
                    "Playlist".into(),
                    false,
                    false,
                    true,
                    false,
                    false,
                )
            } else if browse.starts_with("UC") {
                let is_profile = groups.first().map(String::as_str) == Some("Profile")
                    || groups.get(1).is_some_and(|g| g.starts_with('@'));
                if is_profile {
                    (
                        RowKind::Profile,
                        browse.to_string(),
                        groups.get(1).cloned().unwrap_or_default(),
                        "Profile".into(),
                        false,
                        false,
                        false,
                        false,
                        true,
                    )
                } else {
                    (
                        RowKind::Artist,
                        browse.to_string(),
                        "Artist".into(),
                        "Artist".into(),
                        false,
                        true,
                        false,
                        false,
                        false,
                    )
                }
            } else {
                return None;
            }
        } else {
            let id = row_video_id(r)?;
            let vtype = row_video_type(r);
            if !is_song_video(vtype) {
                return None;
            }
            let rest = without_kind(&groups);
            let artist = rest.first().cloned().unwrap_or_default();
            let is_video = vtype != "MUSIC_VIDEO_TYPE_ATV";
            let album = if !is_video && rest.len() >= 3 { rest[1].clone() } else { String::new() };
            (
                if is_video { RowKind::Video } else { RowKind::Song },
                id.to_string(),
                artist,
                album,
                false,
                false,
                false,
                is_video,
                false,
            )
        };
    Some((
        kind,
        Track {
            id,
            title,
            artist,
            album,
            duration,
            thumbnail,
            is_album,
            is_artist,
            is_playlist,
            is_video,
            is_profile,
            artist_id,
        },
    ))
}

fn search_params(filter: &str) -> &'static str {
    match filter {
        "songs" | "song" => SEARCH_SONGS,
        "videos" | "video" => SEARCH_VIDEOS,
        "albums" | "album" => SEARCH_ALBUMS,
        "artists" | "artist" => SEARCH_ARTISTS,
        "community" | "community_playlist" | "community_playlists" => SEARCH_COMMUNITY,
        "playlists" | "playlist" => SEARCH_PLAYLISTS,
        "profiles" | "profile" => SEARCH_PROFILES,
        _ => SEARCH_ALL,
    }
}

fn filter_kind(filter: &str) -> Option<RowKind> {
    match filter {
        "songs" | "song" => Some(RowKind::Song),
        "videos" | "video" => Some(RowKind::Video),
        "albums" | "album" => Some(RowKind::Album),
        "artists" | "artist" => Some(RowKind::Artist),
        "community" | "community_playlist" | "community_playlists" | "playlists" | "playlist" => {
            Some(RowKind::Playlist)
        }
        "profiles" | "profile" => Some(RowKind::Profile),
        _ => None,
    }
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
        Self {
            http: Client::builder()
                .cookie_store(true)
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("http client"),
            streams: Mutex::new(HashMap::new()),
            visitor: Mutex::new(None),
        }
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

    pub async fn search_sections(&self, query: &str, filter: &str) -> Result<Vec<SearchSection>, String> {
        let body = json!({
            "context": web_context(),
            "query": query,
            "params": search_params(filter)
        });
        let v = self.post("search", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "musicResponsiveListItemRenderer", &mut items);
        let parsed: Vec<(RowKind, Track)> = items.into_iter().filter_map(parse_search_row).collect();
        if filter.is_empty() || filter == "all" {
            let mut sections = vec![];
            for kind in [
                RowKind::Song,
                RowKind::Video,
                RowKind::Album,
                RowKind::Artist,
                RowKind::Playlist,
                RowKind::Profile,
            ] {
                let tracks = dedupe(
                    parsed
                        .iter()
                        .filter(|(k, _)| *k == kind)
                        .map(|(_, t)| t.clone())
                        .collect(),
                );
                if !tracks.is_empty() {
                    sections.push(SearchSection { title: kind.title().into(), kind: kind.key().into(), tracks });
                }
            }
            return Ok(sections);
        }
        let Some(kind) = filter_kind(filter) else { return Ok(vec![]) };
        let tracks = dedupe(
            parsed
                .into_iter()
                .filter(|(k, _)| *k == kind)
                .map(|(_, t)| t)
                .collect(),
        );
        if tracks.is_empty() {
            return Ok(vec![]);
        }
        let title = match filter {
            "community" | "community_playlist" | "community_playlists" => "Community Playlist",
            "playlists" | "playlist" => "Playlist",
            "artists" | "artist" => "Artist",
            "profiles" | "profile" => "Profiles",
            _ => kind.title(),
        };
        Ok(vec![SearchSection { title: title.into(), kind: kind.key().into(), tracks }])
    }

    pub async fn playlists(&self, query: &str) -> Result<Vec<Track>, String> {
        let body = json!({ "context": web_context(), "query": query, "params": PLAYLISTS_FILTER });
        let v = self.post("search", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "musicResponsiveListItemRenderer", &mut items);
        Ok(dedupe(items.into_iter().filter_map(parse_playlist_row).collect()))
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

    pub async fn album(&self, id: &str) -> Result<AlbumPage, String> {
        let body = json!({ "context": web_context(), "browseId": id });
        let v = self.post("browse", body, WEB_UA, true).await?;
        parse_album(&v, id)
    }

    pub async fn album_more(&self, token: &str) -> Result<(Vec<Track>, Option<String>), String> {
        let body = json!({ "context": web_context(), "continuation": token });
        let v = self.post("browse", body, WEB_UA, true).await?;
        let mut items = vec![];
        collect(&v, "musicResponsiveListItemRenderer", &mut items);
        let tracks = items.into_iter().filter_map(parse_list_item).collect();
        Ok((tracks, playlist_continuation(&v)))
    }

    pub async fn artist(&self, id: &str) -> Result<ArtistPage, String> {
        let body = json!({ "context": web_context(), "browseId": id });
        let v = self.post("browse", body, WEB_UA, true).await?;
        let mut immersive = vec![];
        collect(&v, "musicImmersiveHeaderRenderer", &mut immersive);
        let mut visual = vec![];
        collect(&v, "musicVisualHeaderRenderer", &mut visual);
        let is_profile = immersive.is_empty() && !visual.is_empty();
        let name = immersive
            .first()
            .or_else(|| visual.first())
            .and_then(|h| h.pointer("/title/runs/0/text"))
            .and_then(Value::as_str)
            .unwrap_or("Artist")
            .to_string();
        let thumbnail = immersive
            .first()
            .map(|h| thumbnail(h, "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"))
            .or_else(|| visual.first().map(|h| thumbnail(h, "/foregroundThumbnail/musicThumbnailRenderer/thumbnail/thumbnails")))
            .unwrap_or_default();
        let mut carousels = vec![];
        collect(&v, "musicCarouselShelfRenderer", &mut carousels);
        let mut page = ArtistPage {
            name,
            thumbnail,
            top: Vec::new(),
            albums: Vec::new(),
            singles: Vec::new(),
            is_profile,
        };
        for c in &carousels {
            let title = c
                .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            let mut songs = vec![];
            collect(c, "musicResponsiveListItemRenderer", &mut songs);
            let mut cards = vec![];
            collect(c, "musicTwoRowItemRenderer", &mut cards);
            if is_profile && title.contains("video") {
                page.top.extend(cards.into_iter().filter_map(parse_two_row));
            } else if is_profile && title.contains("playlist") {
                page.albums.extend(cards.into_iter().filter_map(parse_two_row));
            } else if title.contains("song") || title.contains("track") {
                page.top.extend(songs.into_iter().filter_map(parse_list_item));
            } else if title.contains("album") {
                page.albums.extend(cards.into_iter().filter_map(parse_two_row));
            } else if title.contains("single") || title.contains("ep") {
                page.singles.extend(cards.into_iter().filter_map(parse_two_row));
            }
        }
        page.top = dedupe(page.top);
        if page.top.is_empty() && page.albums.is_empty() && page.singles.is_empty() {
            return Err("empty artist".into());
        }
        Ok(page)
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
        let key = format!("{id}:{quality}");
        {
            let mut cache = self.streams.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = cache.get(&key) {
                if s.at.elapsed() < STREAM_TTL {
                    return Ok((s.url.clone(), s.duration, s.ua));
                }
            }
            cache.remove(&key);
        }
        let visitor = {
            let cached = self.visitor.lock().unwrap_or_else(|e| e.into_inner()).clone();
            match cached {
                Some((v, at)) if at.elapsed() < VISITOR_TTL => v,
                _ => {
                    let v = extract_visitor(&self.watch_page(id).await.unwrap_or_default());
                    if !v.is_empty() {
                        *self.visitor.lock().unwrap_or_else(|e| e.into_inner()) = Some((v.clone(), Instant::now()));
                    }
                    v
                }
            }
        };

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
                "auto" => audio.iter().copied().max_by_key(|f| {
                    let opus = f["mimeType"].as_str().is_some_and(|m| m.contains("opus"));
                    (opus, f["bitrate"].as_u64().unwrap_or(0))
                }),
                _ => audio.iter().copied().max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)),
            };
            let pick = pick.or_else(|| audio.iter().copied().max_by_key(|f| f["bitrate"].as_u64().unwrap_or(0)));
            if let Some(f) = pick.and_then(|f| f["url"].as_str()) {
                let url = f.to_string();
                let mut cache = self.streams.lock().unwrap_or_else(|e| e.into_inner());
                cache.retain(|_, s| s.at.elapsed() < STREAM_TTL);
                if cache.len() >= 64 {
                    if let Some(old) = cache.iter().min_by_key(|(_, s)| s.at).map(|(k, _)| k.clone()) {
                        cache.remove(&old);
                    }
                }
                cache.insert(key, Stream { url: url.clone(), duration, ua, at: Instant::now() });
                return Ok((url, duration, ua));
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
        is_artist: false,
        is_playlist: false,
        is_video: false,
        is_profile: false,
        artist_id: None,
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
