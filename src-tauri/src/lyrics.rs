use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

const UA: &str = "Melissm/0.1.0 (https://github.com/ryne/melissm)";

#[derive(Serialize, Default)]
pub struct Lyrics {
    synced: Option<String>,
    plain: Option<String>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn pick(v: &Value) -> Lyrics {
    let field = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
    Lyrics { synced: field("syncedLyrics"), plain: field("plainLyrics") }
}

pub async fn fetch(http: &Client, title: &str, artist: &str, album: &str, duration: f64) -> Result<Lyrics, String> {
    let secs = (duration.round() as i64).to_string();
    let exact = http
        .get("https://lrclib.net/api/get")
        .header("User-Agent", UA)
        .query(&[("track_name", title), ("artist_name", artist), ("album_name", album), ("duration", secs.as_str())])
        .send()
        .await
        .map_err(err)?;
    if exact.status().is_success() {
        let v: Value = exact.json().await.map_err(err)?;
        let l = pick(&v);
        if l.synced.is_some() || l.plain.is_some() {
            return Ok(l);
        }
    }
    let found: Value = http
        .get("https://lrclib.net/api/search")
        .header("User-Agent", UA)
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    Ok(found
        .as_array()
        .and_then(|a| a.iter().map(pick).find(|l| l.synced.is_some() || l.plain.is_some()))
        .unwrap_or_default())
}
