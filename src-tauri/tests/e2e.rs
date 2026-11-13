use melissm::innertube::Yt;

#[tokio::test(flavor = "multi_thread")]
async fn search_returns_tracks() {
    let yt = Yt::new();
    let tracks = yt.search("daft punk").await.expect("search");
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| !t.id.is_empty() && !t.title.is_empty()));
    println!("search ok: {} tracks, first: {}", tracks.len(), tracks[0].title);
}

#[tokio::test(flavor = "multi_thread")]
async fn home_returns_sections() {
    let yt = Yt::new();
    let sections = yt.home().await.expect("home");
    assert!(!sections.is_empty());
    for s in &sections {
        println!("section: {} ({} tracks)", s.title, s.tracks.len());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn album_resolves() {
    let yt = Yt::new();
    let sections = yt.home().await.expect("home");
    let album_id = sections
        .iter()
        .flat_map(|s| s.tracks.iter())
        .find(|t| t.is_album)
        .map(|t| t.id.clone())
        .expect("some album in home");
    let tracks = yt.album(&album_id).await.expect("album");
    assert!(!tracks.is_empty());
    println!("album {}: {} tracks", album_id, tracks.len());
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_audio_resolves_stream() {
    let yt = Yt::new();
    let tracks = yt.search("daft punk one more time").await.expect("search");
    let id = &tracks[0].id;
    let (url, dur, _ua) = yt.resolve_stream(id).await.expect("resolve_stream");
    assert!(url.starts_with("https://"), "bad url: {url}");
    assert!(dur > 30.0, "duration too short: {dur}");
    println!("stream ok: {:.0}s, url: {}...", dur, &url[..60]);
}

#[tokio::test(flavor = "multi_thread")]
async fn gstreamer_plays_resolved_stream() {
    let yt = Yt::new();
    let tracks = yt.search("daft punk one more time").await.expect("search");
    let id = &tracks[0].id;
    let (url, _dur, ua) = yt.resolve_stream(id).await.expect("resolve_stream");

    gstreamer::init().expect("gst init");
    let playbin = gstreamer::ElementFactory::make("playbin")
        .build()
        .expect("playbin");
    playbin.set_property("uri", url.as_str());
    playbin.connect("source-setup", false, move |values| {
        if let Ok(src) = values[1].get::<gstreamer::Element>() {
            if src.has_property("user-agent", Some(gstreamer::glib::types::Type::STRING)) {
                src.set_property("user-agent", ua);
            }
        }
        None
    });
    use gstreamer::prelude::*;
    use gstreamer::State;
    playbin.set_state(State::Playing).expect("set state");

    let mut played = 0.0f64;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        played = playbin
            .query_position::<gstreamer::ClockTime>()
            .map(|p| p.seconds() as f64)
            .unwrap_or(0.0);
        if played > 1.5 {
            break;
        }
    }
    playbin.set_state(State::Null).unwrap();
    assert!(played > 1.5, "playback did not advance, stuck at {played}s");
    println!("gstreamer playback ok: reached {:.1}s", played);
}
