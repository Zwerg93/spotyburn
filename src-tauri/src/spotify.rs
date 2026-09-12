use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

pub const SPOTIFY_API_BASE: &str = "https://api.spotify.com/v1";
pub const SPOTIFY_TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
pub const SPOTIFY_AUTH_URL: &str = "https://accounts.spotify.com/authorize";
pub const SPOTIFY_OAUTH_PORT: u16 = 8888;
pub const SPOTIFY_OAUTH_SCOPES: &str =
    "playlist-read-private playlist-read-collaborative user-library-read user-read-private user-read-email";
const EXPIRY_BUFFER_SECS: u64 = 60;

#[derive(Error, Debug)]
pub enum SpotifyError {
    #[error("Authentication failed: {0}")]
    Auth(String),
    #[error("API request failed (Status {status}): {message}")]
    Api {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("Network/HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid Spotify URL or URI: {0}")]
    InvalidResource(String),
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Parse error: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotifyPlaylistSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub track_count: u32,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotifyAlbumSummary {
    pub id: String,
    pub name: String,
    pub artists: Vec<String>,
    pub total_tracks: u32,
    pub release_date: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpotifySearchResult {
    pub tracks: Vec<SpotifyTrack>,
    pub playlists: Vec<SpotifyPlaylistSummary>,
    pub albums: Vec<SpotifyAlbumSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserProfile {
    pub display_name: String,
    pub id: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotifyTrack {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u64,
    pub track_number: u32,
    pub isrc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyResource {
    Playlist(String),
    Album(String),
    Track(String),
}

pub fn parse_spotify_uri(input: &str) -> Result<SpotifyResource, SpotifyError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SpotifyError::InvalidResource(
            "Input cannot be empty".to_string(),
        ));
    }

    let lower = trimmed.to_lowercase();
    if lower.starts_with("spotify:") {
        let parts: Vec<&str> = trimmed.split(':').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("spotify") {
            if parts.len() == 3 {
                if parts[1].eq_ignore_ascii_case("playlist") {
                    return Ok(SpotifyResource::Playlist(parts[2].to_string()));
                } else if parts[1].eq_ignore_ascii_case("album") {
                    return Ok(SpotifyResource::Album(parts[2].to_string()));
                } else if parts[1].eq_ignore_ascii_case("track") {
                    return Ok(SpotifyResource::Track(parts[2].to_string()));
                }
            } else if parts.len() >= 5
                && parts[1].eq_ignore_ascii_case("user")
                && parts[3].eq_ignore_ascii_case("playlist")
            {
                return Ok(SpotifyResource::Playlist(parts[4].to_string()));
            }
        }
        return Err(SpotifyError::InvalidResource(format!(
            "Unsupported Spotify URI format: {}",
            input
        )));
    }

    if lower.contains("spotify.com/") {
        let clean = trimmed.split('?').next().unwrap_or(trimmed);
        let clean = clean.split('#').next().unwrap_or(clean);
        let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();
        for i in 0..segments.len() {
            if segments[i].eq_ignore_ascii_case("playlist") && i + 1 < segments.len() {
                return Ok(SpotifyResource::Playlist(segments[i + 1].to_string()));
            } else if segments[i].eq_ignore_ascii_case("album") && i + 1 < segments.len() {
                return Ok(SpotifyResource::Album(segments[i + 1].to_string()));
            } else if segments[i].eq_ignore_ascii_case("track") && i + 1 < segments.len() {
                return Ok(SpotifyResource::Track(segments[i + 1].to_string()));
            }
        }
        return Err(SpotifyError::InvalidResource(format!(
            "Could not extract playlist, album, or track ID from URL: {}",
            input
        )));
    }

    // Direct base62 Spotify ID (e.g. "37i9dQZF1DXcBWIGoYBM5M" or any 15-30 alphanumeric char ID)
    let alphanumeric_only = trimmed.chars().all(|c| c.is_ascii_alphanumeric());
    if alphanumeric_only && trimmed.len() >= 15 && trimmed.len() <= 35 {
        return Ok(SpotifyResource::Playlist(trimmed.to_string()));
    }

    Err(SpotifyError::InvalidResource(format!(
        "Unrecognized Spotify URL or URI: {}",
        input
    )))
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct PlaylistTracksResponse {
    #[serde(default)]
    items: Vec<serde_json::Value>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FullTrackItem {
    id: Option<String>,
    name: String,
    artists: Vec<ArtistItem>,
    album: Option<AlbumRef>,
    duration_ms: u64,
    track_number: u32,
    external_ids: Option<ExternalIdsItem>,
}

#[derive(Debug, Deserialize)]
struct AlbumDetailsResponse {
    name: String,
    tracks: AlbumTracksResponse,
}

#[derive(Debug, Deserialize)]
struct AlbumTracksResponse {
    items: Vec<SimplifiedTrackItem>,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SimplifiedTrackItem {
    id: Option<String>,
    name: String,
    artists: Vec<ArtistItem>,
    duration_ms: u64,
    track_number: u32,
    external_ids: Option<ExternalIdsItem>,
}

#[derive(Debug, Deserialize)]
struct ArtistItem {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AlbumRef {
    name: String,
}

#[derive(Debug, Deserialize)]
struct ExternalIdsItem {
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserPlaylistsResponse {
    items: Vec<UserPlaylistItem>,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserPlaylistItem {
    id: String,
    name: String,
    description: Option<String>,
    images: Option<Vec<SpotifyImageItem>>,
    tracks: Option<PlaylistTracksRef>,
    items: Option<PlaylistTracksRef>,
}

#[derive(Debug, Deserialize)]
struct SpotifyImageItem {
    url: String,
    #[allow(dead_code)]
    height: Option<u32>,
    #[allow(dead_code)]
    width: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PlaylistTracksRef {
    total: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CurrentUserResponse {
    id: String,
    display_name: Option<String>,
    images: Option<Vec<SpotifyImageItem>>,
}

#[derive(Debug, Deserialize)]
struct SearchApiResponse {
    #[serde(default)]
    tracks: Option<SearchItemsWrapper<FullTrackItem>>,
    #[serde(default)]
    playlists: Option<SearchItemsWrapper<SearchPlaylistItem>>,
    #[serde(default)]
    albums: Option<SearchItemsWrapper<SearchAlbumItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct SearchItemsWrapper<T> {
    #[serde(default)]
    items: Vec<Option<T>>,
}

#[derive(Debug, Deserialize)]
struct SearchPlaylistItem {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    images: Option<Vec<SpotifyImageItem>>,
    tracks: Option<PlaylistTracksRef>,
}

#[derive(Debug, Deserialize)]
struct SearchAlbumItem {
    id: Option<String>,
    name: Option<String>,
    artists: Option<Vec<ArtistItem>>,
    total_tracks: Option<u32>,
    release_date: Option<String>,
    images: Option<Vec<SpotifyImageItem>>,
}

pub fn build_search_url(base_url: &str, query: &str, types: &[&str], limit: u32) -> String {
    let type_str = if types.is_empty() {
        "track,playlist,album".to_string()
    } else {
        types.join(",")
    };
    // Spotify Web API strictly caps search limit to max 10 (anything > 10 returns 400 Bad Request: "Invalid limit")
    let limit_val = if limit == 0 { 10 } else { limit.clamp(1, 10) };

    let base = format!("{}/search", base_url.trim_end_matches('/'));
    let mut url = reqwest::Url::parse(&base)
        .unwrap_or_else(|_| reqwest::Url::parse("https://api.spotify.com/v1/search").unwrap());
    url.query_pairs_mut()
        .append_pair("q", query.trim())
        .append_pair("type", &type_str)
        .append_pair("limit", &limit_val.to_string());
    url.to_string()
}

pub fn parse_search_response(json_str: &str) -> Result<SpotifySearchResult, SpotifyError> {
    let raw: SearchApiResponse = serde_json::from_str(json_str)?;

    let mut tracks = Vec::new();
    if let Some(track_wrapper) = raw.tracks {
        for track in track_wrapper.items.into_iter().flatten() {
            if let Some(id) = track.id {
                let artists = track.artists.into_iter().map(|a| a.name).collect();
                let album = track.album.map(|a| a.name).unwrap_or_default();
                let isrc = track.external_ids.and_then(|e| e.isrc);
                tracks.push(SpotifyTrack {
                    id,
                    title: track.name,
                    artists,
                    album,
                    duration_ms: track.duration_ms,
                    track_number: track.track_number,
                    isrc,
                });
            }
        }
    }

    let mut playlists = Vec::new();
    if let Some(playlist_wrapper) = raw.playlists {
        for pl in playlist_wrapper.items.into_iter().flatten() {
            if let Some(id) = pl.id {
                let name = pl.name.unwrap_or_else(|| "Untitled Playlist".to_string());
                let track_count = pl.tracks.and_then(|t| t.total).unwrap_or(0);
                let image_url = pl
                    .images
                    .and_then(|imgs| imgs.into_iter().next().map(|i| i.url));

                playlists.push(SpotifyPlaylistSummary {
                    id,
                    name,
                    description: pl.description.filter(|s| !s.is_empty()),
                    track_count,
                    image_url,
                });
            }
        }
    }

    let mut albums = Vec::new();
    if let Some(album_wrapper) = raw.albums {
        for alb in album_wrapper.items.into_iter().flatten() {
            if let Some(id) = alb.id {
                let name = alb.name.unwrap_or_else(|| "Untitled Album".to_string());
                let artists = alb
                    .artists
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| a.name)
                    .collect();
                let total_tracks = alb.total_tracks.unwrap_or(0);
                let image_url = alb
                    .images
                    .and_then(|imgs| imgs.into_iter().next().map(|i| i.url));

                albums.push(SpotifyAlbumSummary {
                    id,
                    name,
                    artists,
                    total_tracks,
                    release_date: alb.release_date,
                    image_url,
                });
            }
        }
    }

    Ok(SpotifySearchResult {
        tracks,
        playlists,
        albums,
    })
}

pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

pub fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

pub fn extract_query_param(request: &str, key: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let k = parts.next()?;
        let v = parts.next().unwrap_or("");
        if k == key {
            return Some(v.to_string());
        }
    }
    None
}

pub fn extract_next_data_json(html: &str) -> Option<&str> {
    let marker = r#"id="__NEXT_DATA__""#;
    let marker_pos = html.find(marker)?;
    let tag_close = html[marker_pos..].find('>')?;
    let json_start = marker_pos + tag_close + 1;
    let json_end = html[json_start..].find("</script>")?;
    Some(html[json_start..json_start + json_end].trim())
}

#[derive(Debug, Deserialize)]
struct EmbedRoot {
    props: Option<EmbedProps>,
}

#[derive(Debug, Deserialize)]
struct EmbedProps {
    #[serde(rename = "pageProps")]
    page_props: Option<EmbedPageProps>,
}

#[derive(Debug, Deserialize)]
struct EmbedPageProps {
    state: Option<EmbedState>,
}

#[derive(Debug, Deserialize)]
struct EmbedState {
    data: Option<EmbedData>,
}

#[derive(Debug, Deserialize)]
struct EmbedData {
    entity: Option<EmbedEntity>,
}

#[derive(Debug, Deserialize)]
struct EmbedEntity {
    #[serde(rename = "type")]
    entity_type: Option<String>,
    name: Option<String>,
    title: Option<String>,
    uri: Option<String>,
    id: Option<String>,
    artists: Option<Vec<EmbedArtist>>,
    subtitle: Option<String>,
    duration: Option<u64>,
    #[serde(rename = "trackList", default)]
    track_list: Vec<EmbedTrackItem>,
}

#[derive(Debug, Deserialize)]
struct EmbedTrackItem {
    uri: Option<String>,
    title: Option<String>,
    name: Option<String>,
    artists: Option<Vec<EmbedArtist>>,
    subtitle: Option<String>,
    album: Option<EmbedAlbumRef>,
    duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct EmbedArtist {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EmbedAlbumRef {
    name: Option<String>,
}

pub fn parse_embed_tracks(html: &str) -> Result<Vec<SpotifyTrack>, SpotifyError> {
    let json_str = extract_next_data_json(html).ok_or_else(|| {
        SpotifyError::Parse("Could not find __NEXT_DATA__ script in embed HTML".to_string())
    })?;

    let root: EmbedRoot = serde_json::from_str(json_str)
        .map_err(|e| SpotifyError::Parse(format!("Failed to parse embed JSON: {}", e)))?;

    let entity = root
        .props
        .and_then(|p| p.page_props)
        .and_then(|pp| pp.state)
        .and_then(|s| s.data)
        .and_then(|d| d.entity)
        .ok_or_else(|| SpotifyError::Parse("Missing entity in embed JSON".to_string()))?;

    let entity_type = entity.entity_type.unwrap_or_default();
    let album_or_playlist_name = entity
        .name
        .as_deref()
        .or(entity.title.as_deref())
        .unwrap_or_default()
        .to_string();

    let mut tracks = Vec::new();

    if entity_type == "track" {
        let uri = entity.uri.unwrap_or_default();
        let id = entity.id.unwrap_or_else(|| {
            uri.strip_prefix("spotify:track:")
                .unwrap_or(&uri)
                .to_string()
        });
        let title = entity
            .title
            .or(entity.name)
            .unwrap_or_else(|| "Unknown Track".to_string());

        let mut artists = Vec::new();
        if let Some(artist_arr) = entity.artists {
            for a in artist_arr {
                if let Some(name) = a.name {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        artists.push(trimmed.to_string());
                    }
                }
            }
        }
        if artists.is_empty() {
            if let Some(subtitle) = entity.subtitle {
                let clean = subtitle.replace('\u{00A0}', " ");
                artists = clean
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        let duration_ms = entity.duration.unwrap_or(0);
        tracks.push(SpotifyTrack {
            id,
            title,
            artists,
            album: album_or_playlist_name,
            duration_ms,
            track_number: 1,
            isrc: None,
        });
        return Ok(tracks);
    }

    for (idx, item) in entity.track_list.into_iter().enumerate() {
        let uri = item.uri.unwrap_or_default();
        let id = uri
            .strip_prefix("spotify:track:")
            .unwrap_or(&uri)
            .trim()
            .to_string();
        if id.is_empty() {
            continue;
        }
        let title = item
            .title
            .or(item.name)
            .unwrap_or_else(|| "Unknown Track".to_string());

        let mut artists = Vec::new();
        if let Some(arr) = item.artists {
            for a in arr {
                if let Some(name) = a.name {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        artists.push(trimmed.to_string());
                    }
                }
            }
        }
        if artists.is_empty() {
            if let Some(subtitle) = item.subtitle {
                let clean = subtitle.replace('\u{00A0}', " ");
                artists = clean
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        let album = item
            .album
            .and_then(|a| a.name)
            .unwrap_or_else(|| album_or_playlist_name.clone());

        let duration_ms = item.duration.unwrap_or(0);

        tracks.push(SpotifyTrack {
            id,
            title,
            artists,
            album,
            duration_ms,
            track_number: (idx + 1) as u32,
            isrc: None,
        });
    }

    if tracks.is_empty() {
        return Err(SpotifyError::Parse(
            "No tracks found in embed entity".to_string(),
        ));
    }

    Ok(tracks)
}

pub async fn start_oauth_loopback(
    client_id: &str,
    client_secret: &str,
    port: u16,
) -> Result<OAuthTokens, SpotifyError> {
    start_oauth_loopback_internal(
        client_id,
        client_secret,
        port,
        SPOTIFY_AUTH_URL,
        SPOTIFY_TOKEN_URL,
        true,
    )
    .await
}

pub async fn start_oauth_loopback_internal(
    client_id: &str,
    client_secret: &str,
    port: u16,
    auth_base_url: &str,
    token_url: &str,
    open_browser_flag: bool,
) -> Result<OAuthTokens, SpotifyError> {
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .map_err(|e| {
            SpotifyError::Auth(format!(
                "Failed to bind TCP listener on port {}: {}",
                port, e
            ))
        })?;

    start_oauth_loopback_with_listener(
        listener,
        client_id,
        client_secret,
        auth_base_url,
        token_url,
        open_browser_flag,
    )
    .await
}

pub async fn start_oauth_loopback_with_listener(
    listener: tokio::net::TcpListener,
    client_id: &str,
    client_secret: &str,
    auth_base_url: &str,
    token_url: &str,
    open_browser_flag: bool,
) -> Result<OAuthTokens, SpotifyError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let port = listener.local_addr().map(|a| a.port()).unwrap_or(8888);
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);
    let auth_url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}",
        auth_base_url,
        client_id,
        url_encode(&redirect_uri),
        url_encode(SPOTIFY_OAUTH_SCOPES)
    );

    if open_browser_flag {
        let _ = open_browser(&auth_url);
    }

    let (mut socket, _) = listener.accept().await.map_err(|e| {
        SpotifyError::Auth(format!("Failed to accept incoming OAuth callback: {}", e))
    })?;

    let mut buf = [0u8; 4096];
    let n = socket
        .read(&mut buf)
        .await
        .map_err(|e| SpotifyError::Auth(format!("Failed to read OAuth callback request: {}", e)))?;

    let req_text = String::from_utf8_lossy(&buf[..n]);

    if let Some(err_code) = extract_query_param(&req_text, "error") {
        let err_desc = extract_query_param(&req_text, "error_description").unwrap_or_default();
        return Err(SpotifyError::Auth(format!(
            "Spotify authorization denied: {} {}",
            err_code, err_desc
        )));
    }

    let code = extract_query_param(&req_text, "code").ok_or_else(|| {
        SpotifyError::Auth("No authorization code found in callback request".to_string())
    })?;

    let html = r#"<!DOCTYPE html>
<html lang="de">
<head>
  <meta charset="UTF-8">
  <title>SpotyBurn - Anmeldung erfolgreich</title>
  <style>
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
      background-color: #121212;
      color: #ffffff;
      display: flex;
      justify-content: center;
      align-items: center;
      height: 100vh;
      margin: 0;
    }
    .card {
      background: #181818;
      border: 1px solid #282828;
      border-radius: 12px;
      padding: 40px;
      text-align: center;
      max-width: 440px;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    }
    .icon {
      font-size: 48px;
      color: #1db954;
      margin-bottom: 16px;
    }
    h1 {
      font-size: 22px;
      font-weight: 700;
      margin: 0 0 12px;
      color: #ffffff;
    }
    p {
      font-size: 14px;
      color: #a7a7a7;
      line-height: 1.5;
      margin: 0;
    }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">&#10004;</div>
    <h1>Anmeldung bei SpotyBurn erfolgreich!</h1>
    <p>Anmeldung bei SpotyBurn erfolgreich! Du kannst diesen Tab schließen.</p>
  </div>
</body>
</html>"#;

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );

    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.flush().await;
    let _ = socket.shutdown().await;
    drop(socket);
    drop(listener);

    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(SpotifyError::Auth(format!(
            "OAuth token exchange failed with status {}: {}",
            status, body
        )));
    }

    let auth_data: AuthTokenResponse = resp.json().await?;
    let refresh_token = auth_data.refresh_token.ok_or_else(|| {
        SpotifyError::Auth("No refresh_token returned by Spotify OAuth token exchange".to_string())
    })?;

    Ok(OAuthTokens {
        access_token: auth_data.access_token,
        refresh_token,
        expires_in: auth_data.expires_in,
    })
}

pub async fn refresh_access_token(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<OAuthTokens, SpotifyError> {
    let client = reqwest::Client::new();
    refresh_access_token_internal(
        &client,
        SPOTIFY_TOKEN_URL,
        client_id,
        client_secret,
        refresh_token,
    )
    .await
}

pub async fn refresh_access_token_internal(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<OAuthTokens, SpotifyError> {
    let resp = client
        .post(token_url)
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(SpotifyError::Auth(format!(
            "Token refresh failed with status {}: {}",
            status, body
        )));
    }

    let token_data: RefreshTokenResponse = resp.json().await?;
    let new_refresh_token = token_data
        .refresh_token
        .unwrap_or_else(|| refresh_token.to_string());

    Ok(OAuthTokens {
        access_token: token_data.access_token,
        refresh_token: new_refresh_token,
        expires_in: token_data.expires_in,
    })
}

#[derive(Clone)]
pub struct SpotifyClient {
    client: reqwest::Client,
    client_id: String,
    client_secret: String,
    refresh_token: Arc<RwLock<Option<String>>>,
    user_access_token: Arc<RwLock<Option<String>>>,
    token: Arc<RwLock<Option<CachedToken>>>,
    api_base_url: String,
    token_url: String,
}

impl SpotifyClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::with_base_urls(
            client_id,
            client_secret,
            SPOTIFY_API_BASE.to_string(),
            SPOTIFY_TOKEN_URL.to_string(),
        )
    }

    pub fn with_refresh_token(
        client_id: String,
        client_secret: String,
        refresh_token: String,
    ) -> Self {
        let client = Self::new(client_id, client_secret);
        let rt = client.refresh_token.clone();
        if let Ok(mut lock) = rt.try_write() {
            *lock = Some(refresh_token);
        }
        client
    }

    pub fn with_user_token(
        client_id: String,
        client_secret: String,
        user_access_token: Option<String>,
    ) -> Self {
        let client = Self::new(client_id, client_secret);
        if let Ok(mut lock) = client.user_access_token.try_write() {
            *lock = user_access_token;
        }
        client
    }

    pub fn with_tokens(
        client_id: String,
        client_secret: String,
        refresh_token: Option<String>,
        user_access_token: Option<String>,
    ) -> Self {
        let client = Self::new(client_id, client_secret);
        if let Some(rt) = refresh_token {
            if let Ok(mut lock) = client.refresh_token.try_write() {
                *lock = Some(rt);
            }
        }
        if let Ok(mut lock) = client.user_access_token.try_write() {
            *lock = user_access_token;
        }
        client
    }

    pub fn with_base_urls(
        client_id: String,
        client_secret: String,
        api_base_url: String,
        token_url: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            client_id,
            client_secret,
            refresh_token: Arc::new(RwLock::new(None)),
            user_access_token: Arc::new(RwLock::new(None)),
            token: Arc::new(RwLock::new(None)),
            api_base_url,
            token_url,
        }
    }

    pub async fn set_user_access_token(&self, token: Option<String>) {
        let mut lock = self.user_access_token.write().await;
        *lock = token;
    }

    pub async fn get_user_access_token(&self) -> Option<String> {
        let lock = self.user_access_token.read().await;
        lock.clone()
    }

    pub async fn get_effective_token(&self) -> Result<String, SpotifyError> {
        {
            let guard = self.user_access_token.read().await;
            if let Some(token) = &*guard {
                let trimmed = token.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
        self.authenticate().await
    }

    pub async fn authenticate_client_credentials(&self) -> Result<String, SpotifyError> {
        let resp = self
            .client
            .post(&self.token_url)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Auth(format!(
                "Token request failed with status {}: {}",
                status, body
            )));
        }

        let token_data: TokenResponse = resp.json().await?;
        Ok(token_data.access_token)
    }

    pub async fn set_refresh_token(&self, token: Option<String>) {
        let mut lock = self.refresh_token.write().await;
        *lock = token;
        let mut t_lock = self.token.write().await;
        *t_lock = None;
    }

    pub async fn get_refresh_token(&self) -> Option<String> {
        let lock = self.refresh_token.read().await;
        lock.clone()
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokens, SpotifyError> {
        refresh_access_token_internal(
            &self.client,
            &self.token_url,
            &self.client_id,
            &self.client_secret,
            refresh_token,
        )
        .await
    }

    pub async fn start_oauth_loopback(&self, port: u16) -> Result<OAuthTokens, SpotifyError> {
        start_oauth_loopback(&self.client_id, &self.client_secret, port).await
    }

    pub async fn authenticate(&self) -> Result<String, SpotifyError> {
        // Check read lock
        {
            let guard = self.token.read().await;
            if let Some(cached) = &*guard {
                if Instant::now() + Duration::from_secs(EXPIRY_BUFFER_SECS) < cached.expires_at {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        // Acquire write lock and refresh
        let mut guard = self.token.write().await;
        // Double check after obtaining write lock
        if let Some(cached) = &*guard {
            if Instant::now() + Duration::from_secs(EXPIRY_BUFFER_SECS) < cached.expires_at {
                return Ok(cached.access_token.clone());
            }
        }

        let maybe_refresh = {
            let rt_guard = self.refresh_token.read().await;
            rt_guard.clone()
        };

        if let Some(ref rt) = maybe_refresh {
            let tokens = refresh_access_token_internal(
                &self.client,
                &self.token_url,
                &self.client_id,
                &self.client_secret,
                rt,
            )
            .await?;

            if tokens.refresh_token != *rt {
                let mut rt_guard = self.refresh_token.write().await;
                *rt_guard = Some(tokens.refresh_token);
            }

            let expires_at = Instant::now() + Duration::from_secs(tokens.expires_in);
            *guard = Some(CachedToken {
                access_token: tokens.access_token.clone(),
                expires_at,
            });

            return Ok(tokens.access_token);
        }

        let resp = self
            .client
            .post(&self.token_url)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Auth(format!(
                "Token request failed with status {}: {}",
                status, body
            )));
        }

        let token_data: TokenResponse = resp.json().await?;
        let expires_at = Instant::now() + Duration::from_secs(token_data.expires_in);
        *guard = Some(CachedToken {
            access_token: token_data.access_token.clone(),
            expires_at,
        });

        Ok(token_data.access_token)
    }

    pub async fn fetch(&self, input: &str) -> Result<Vec<SpotifyTrack>, SpotifyError> {
        let resource = parse_spotify_uri(input)?;
        match resource {
            SpotifyResource::Playlist(id) => self.fetch_playlist(&id).await,
            SpotifyResource::Album(id) => self.fetch_album(&id).await,
            SpotifyResource::Track(id) => {
                let track = self.fetch_track(&id).await?;
                Ok(vec![track])
            }
        }
    }

    pub async fn fetch_playlist_embed(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<SpotifyTrack>, SpotifyError> {
        let url = format!("https://open.spotify.com/embed/playlist/{}", playlist_id);
        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SpotifyError::Api {
                status: resp.status(),
                message: format!("Failed to fetch embed page for playlist {}", playlist_id),
            });
        }
        let html = resp.text().await?;
        parse_embed_tracks(&html)
    }

    pub async fn fetch_album_embed(
        &self,
        album_id: &str,
    ) -> Result<Vec<SpotifyTrack>, SpotifyError> {
        let url = format!("https://open.spotify.com/embed/album/{}", album_id);
        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SpotifyError::Api {
                status: resp.status(),
                message: format!("Failed to fetch embed page for album {}", album_id),
            });
        }
        let html = resp.text().await?;
        parse_embed_tracks(&html)
    }

    pub async fn fetch_track_embed(&self, track_id: &str) -> Result<SpotifyTrack, SpotifyError> {
        let url = format!("https://open.spotify.com/embed/track/{}", track_id);
        let resp = self
            .client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(SpotifyError::Api {
                status: resp.status(),
                message: format!("Failed to fetch embed page for track {}", track_id),
            });
        }
        let html = resp.text().await?;
        let tracks = parse_embed_tracks(&html)?;
        tracks
            .into_iter()
            .next()
            .ok_or_else(|| SpotifyError::NotFound(format!("Track {} not found in embed", track_id)))
    }

    pub async fn fetch_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<SpotifyTrack>, SpotifyError> {
        let mut token = match self.get_effective_token().await {
            Ok(t) => t,
            Err(_) => return self.fetch_playlist_embed(playlist_id).await,
        };
        let mut tracks = Vec::new();
        // Try the modern Spotify /items endpoint first, fallback to /tracks
        let mut next_url = Some(format!(
            "{}/playlists/{}/items?limit=100&offset=0",
            self.api_base_url, playlist_id
        ));
        let mut tried_tracks_endpoint = false;

        while let Some(url) = next_url {
            let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;

            if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                if let Ok(new_token) = self.authenticate().await {
                    token = new_token;
                    resp = self.client.get(&url).bearer_auth(&token).send().await?;
                }
            }

            let status = resp.status();
            if (status == reqwest::StatusCode::NOT_FOUND
                || status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::BAD_REQUEST)
                && !tried_tracks_endpoint
            {
                // Fallback to older /tracks endpoint before embed
                tried_tracks_endpoint = true;
                next_url = Some(format!(
                    "{}/playlists/{}/tracks?limit=100&offset=0&additional_types=track",
                    self.api_base_url, playlist_id
                ));
                continue;
            }

            if status == reqwest::StatusCode::FORBIDDEN {
                return self.fetch_playlist_embed(playlist_id).await;
            }
            if status == reqwest::StatusCode::NOT_FOUND {
                if let Ok(embed_tracks) = self.fetch_playlist_embed(playlist_id).await {
                    if !embed_tracks.is_empty() {
                        return Ok(embed_tracks);
                    }
                }
                return Err(SpotifyError::NotFound(format!(
                    "Playlist {} not found",
                    playlist_id
                )));
            }
            if !status.is_success() {
                if let Ok(embed_tracks) = self.fetch_playlist_embed(playlist_id).await {
                    if !embed_tracks.is_empty() {
                        return Ok(embed_tracks);
                    }
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(SpotifyError::Api {
                    status,
                    message: body,
                });
            }

            let page: PlaylistTracksResponse = resp.json().await?;
            for item_val in page.items {
                let track_obj = if item_val.get("item").is_some() && !item_val["item"].is_null() {
                    &item_val["item"]
                } else if item_val.get("track").is_some() && !item_val["track"].is_null() {
                    &item_val["track"]
                } else {
                    &item_val
                };

                if track_obj.is_null() {
                    continue;
                }

                let id = match track_obj.get("id").and_then(|v| v.as_str()) {
                    Some(s) if !s.trim().is_empty() => s.trim().to_string(),
                    _ => continue,
                };

                let title = track_obj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown Track")
                    .to_string();

                let artists: Vec<String> = track_obj
                    .get("artists")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| {
                                a.get("name")
                                    .and_then(|n| n.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let album = track_obj
                    .get("album")
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();

                let duration_ms = track_obj
                    .get("duration_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let track_number = track_obj
                    .get("track_number")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u32;

                let isrc = track_obj
                    .get("external_ids")
                    .and_then(|e| e.get("isrc"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                tracks.push(SpotifyTrack {
                    id,
                    title,
                    artists,
                    album,
                    duration_ms,
                    track_number,
                    isrc,
                });
            }

            next_url = page.next;
        }

        if tracks.is_empty() {
            if let Ok(embed_tracks) = self.fetch_playlist_embed(playlist_id).await {
                if !embed_tracks.is_empty() {
                    return Ok(embed_tracks);
                }
            }
        }

        Ok(tracks)
    }

    pub async fn fetch_album(&self, album_id: &str) -> Result<Vec<SpotifyTrack>, SpotifyError> {
        let mut token = match self.get_effective_token().await {
            Ok(t) => t,
            Err(_) => return self.fetch_album_embed(album_id).await,
        };
        let url = format!("{}/albums/{}", self.api_base_url, album_id);

        let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(new_token) = self.authenticate().await {
                token = new_token;
                resp = self.client.get(&url).bearer_auth(&token).send().await?;
            }
        }

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return self.fetch_album_embed(album_id).await;
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            if let Ok(embed_tracks) = self.fetch_album_embed(album_id).await {
                if !embed_tracks.is_empty() {
                    return Ok(embed_tracks);
                }
            }
            return Err(SpotifyError::NotFound(format!(
                "Album {} not found",
                album_id
            )));
        }
        if !status.is_success() {
            if let Ok(embed_tracks) = self.fetch_album_embed(album_id).await {
                if !embed_tracks.is_empty() {
                    return Ok(embed_tracks);
                }
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Api {
                status,
                message: body,
            });
        }

        let album_data: AlbumDetailsResponse = resp.json().await?;
        let album_name = album_data.name;
        let mut tracks = Vec::new();

        for track in album_data.tracks.items {
            if let Some(id) = track.id {
                let artists = track.artists.into_iter().map(|a| a.name).collect();
                let isrc = track.external_ids.and_then(|e| e.isrc);
                tracks.push(SpotifyTrack {
                    id,
                    title: track.name,
                    artists,
                    album: album_name.clone(),
                    duration_ms: track.duration_ms,
                    track_number: track.track_number,
                    isrc,
                });
            }
        }

        // Follow pagination if album has > 50 tracks
        let mut next_url = album_data.tracks.next;
        while let Some(url) = next_url {
            let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                if let Ok(new_token) = self.authenticate().await {
                    token = new_token;
                    resp = self.client.get(&url).bearer_auth(&token).send().await?;
                }
            }

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(SpotifyError::Api {
                    status,
                    message: body,
                });
            }

            let page: AlbumTracksResponse = resp.json().await?;
            for track in page.items {
                if let Some(id) = track.id {
                    let artists = track.artists.into_iter().map(|a| a.name).collect();
                    let isrc = track.external_ids.and_then(|e| e.isrc);
                    tracks.push(SpotifyTrack {
                        id,
                        title: track.name,
                        artists,
                        album: album_name.clone(),
                        duration_ms: track.duration_ms,
                        track_number: track.track_number,
                        isrc,
                    });
                }
            }

            next_url = page.next;
        }

        if tracks.is_empty() {
            if let Ok(embed_tracks) = self.fetch_album_embed(album_id).await {
                if !embed_tracks.is_empty() {
                    return Ok(embed_tracks);
                }
            }
        }

        Ok(tracks)
    }

    pub async fn fetch_track(&self, track_id: &str) -> Result<SpotifyTrack, SpotifyError> {
        let mut token = match self.get_effective_token().await {
            Ok(t) => t,
            Err(_) => return self.fetch_track_embed(track_id).await,
        };
        let url = format!("{}/tracks/{}", self.api_base_url, track_id);

        let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(new_token) = self.authenticate().await {
                token = new_token;
                resp = self.client.get(&url).bearer_auth(&token).send().await?;
            }
        }

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return self.fetch_track_embed(track_id).await;
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            if let Ok(track) = self.fetch_track_embed(track_id).await {
                return Ok(track);
            }
            return Err(SpotifyError::NotFound(format!(
                "Track {} not found",
                track_id
            )));
        }
        if !status.is_success() {
            if let Ok(track) = self.fetch_track_embed(track_id).await {
                return Ok(track);
            }
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Api {
                status,
                message: body,
            });
        }

        let track: FullTrackItem = resp.json().await?;
        let id = track.id.unwrap_or_else(|| track_id.to_string());
        let artists = track.artists.into_iter().map(|a| a.name).collect();
        let album = track.album.map(|a| a.name).unwrap_or_default();
        let isrc = track.external_ids.and_then(|e| e.isrc);

        Ok(SpotifyTrack {
            id,
            title: track.name,
            artists,
            album,
            duration_ms: track.duration_ms,
            track_number: track.track_number,
            isrc,
        })
    }

    pub async fn fetch_user_playlists(&self) -> Result<Vec<SpotifyPlaylistSummary>, SpotifyError> {
        let mut token = self.get_effective_token().await?;
        let mut playlists = Vec::new();
        let mut next_url = Some(format!("{}/me/playlists?limit=50", self.api_base_url));

        while let Some(url) = next_url {
            let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                if let Ok(new_token) = self.authenticate().await {
                    token = new_token;
                    resp = self.client.get(&url).bearer_auth(&token).send().await?;
                }
            }

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(SpotifyError::Api {
                    status,
                    message: body,
                });
            }

            let page: UserPlaylistsResponse = resp.json().await?;
            for item in page.items {
                let image_url = item
                    .images
                    .and_then(|imgs| imgs.into_iter().next().map(|i| i.url));
                let track_count = item
                    .items
                    .as_ref()
                    .or(item.tracks.as_ref())
                    .and_then(|t| t.total)
                    .unwrap_or(0);
                let description = item.description.filter(|s| !s.is_empty());

                playlists.push(SpotifyPlaylistSummary {
                    id: item.id,
                    name: item.name,
                    description,
                    track_count,
                    image_url,
                });
            }

            next_url = page.next;
        }

        Ok(playlists)
    }

    pub async fn fetch_current_user_profile(&self) -> Result<UserProfile, SpotifyError> {
        let mut token = self.get_effective_token().await?;
        let url = format!("{}/me", self.api_base_url);

        let mut resp = self.client.get(&url).bearer_auth(&token).send().await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(new_token) = self.authenticate().await {
                token = new_token;
                resp = self.client.get(&url).bearer_auth(&token).send().await?;
            }
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Api {
                status,
                message: body,
            });
        }

        let user_data: CurrentUserResponse = resp.json().await?;
        let display_name = user_data
            .display_name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| user_data.id.clone());
        let image_url = user_data
            .images
            .and_then(|imgs| imgs.into_iter().next().map(|i| i.url));

        Ok(UserProfile {
            id: user_data.id,
            display_name,
            image_url,
        })
    }

    pub async fn search(
        &self,
        query: &str,
        types: &[&str],
        limit: u32,
    ) -> Result<SpotifySearchResult, SpotifyError> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Ok(SpotifySearchResult::default());
        }

        let search_url = build_search_url(&self.api_base_url, trimmed_query, types, limit);
        let effective_token = self.get_effective_token().await?;

        let mut resp = self
            .client
            .get(&search_url)
            .bearer_auth(&effective_token)
            .send()
            .await?;

        // Dual-token fallback: If user access token / user auth was rejected (401),
        // fallback to client-credentials token so search always succeeds!
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            if let Ok(client_token) = self.authenticate_client_credentials().await {
                if client_token != effective_token {
                    resp = self
                        .client
                        .get(&search_url)
                        .bearer_auth(&client_token)
                        .send()
                        .await?;
                }
            }
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SpotifyError::Api {
                status,
                message: body,
            });
        }

        let body = resp.text().await?;
        parse_search_response(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn test_parse_spotify_uri_uris() {
        assert_eq!(
            parse_spotify_uri("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap(),
            SpotifyResource::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );

        assert_eq!(
            parse_spotify_uri("spotify:album:4aawyAB9vmqN3uQ7FjRGTy").unwrap(),
            SpotifyResource::Album("4aawyAB9vmqN3uQ7FjRGTy".to_string())
        );

        assert_eq!(
            parse_spotify_uri("spotify:track:4cOdK2wGLETKBW3PvgPWqT").unwrap(),
            SpotifyResource::Track("4cOdK2wGLETKBW3PvgPWqT".to_string())
        );

        assert_eq!(
            parse_spotify_uri("spotify:user:spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").unwrap(),
            SpotifyResource::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );
    }

    #[test]
    fn test_parse_spotify_uri_urls() {
        assert_eq!(
            parse_spotify_uri("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M").unwrap(),
            SpotifyResource::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );

        assert_eq!(
            parse_spotify_uri(
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abcd1234efgh"
            )
            .unwrap(),
            SpotifyResource::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );

        assert_eq!(
            parse_spotify_uri("https://open.spotify.com/intl-de/playlist/37i9dQZF1DXcBWIGoYBM5M")
                .unwrap(),
            SpotifyResource::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string())
        );

        assert_eq!(
            parse_spotify_uri("https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy").unwrap(),
            SpotifyResource::Album("4aawyAB9vmqN3uQ7FjRGTy".to_string())
        );

        assert_eq!(
            parse_spotify_uri("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=xyz")
                .unwrap(),
            SpotifyResource::Track("4cOdK2wGLETKBW3PvgPWqT".to_string())
        );
    }

    #[test]
    fn test_parse_spotify_uri_invalid() {
        assert!(parse_spotify_uri("").is_err());
        assert!(parse_spotify_uri("   ").is_err());
        assert!(parse_spotify_uri("https://youtube.com/watch?v=123").is_err());
        assert!(parse_spotify_uri("spotify:unknown:123").is_err());
    }

    #[test]
    fn test_track_deserialization() {
        let json = r#"{
            "id": "4cOdK2wGLETKBW3PvgPWqT",
            "name": "Never Gonna Give You Up",
            "artists": [{"name": "Rick Astley"}],
            "album": {"name": "Whenever You Need Somebody"},
            "duration_ms": 213573,
            "track_number": 1,
            "external_ids": {"isrc": "GBARL8700012"}
        }"#;

        let item: FullTrackItem = serde_json::from_str(json).expect("deserialize track");
        assert_eq!(item.id.as_deref(), Some("4cOdK2wGLETKBW3PvgPWqT"));
        assert_eq!(item.name, "Never Gonna Give You Up");
        assert_eq!(item.artists[0].name, "Rick Astley");
        assert_eq!(item.album.unwrap().name, "Whenever You Need Somebody");
        assert_eq!(item.duration_ms, 213573);
        assert_eq!(item.track_number, 1);
        assert_eq!(
            item.external_ids.unwrap().isrc.as_deref(),
            Some("GBARL8700012")
        );
    }

    #[test]
    fn test_token_deserialization() {
        let json = r#"{
            "access_token": "mock_token_123",
            "token_type": "Bearer",
            "expires_in": 3600
        }"#;

        let token: TokenResponse = serde_json::from_str(json).expect("deserialize token");
        assert_eq!(token.access_token, "mock_token_123");
        assert_eq!(token.expires_in, 3600);
    }

    #[tokio::test]
    async fn test_spotify_client_auth_caching_and_pagination() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let token_url = format!("http://{}/api/token", addr);

        let auth_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let auth_count_clone = auth_count.clone();

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                let auth_counter = auth_count_clone.clone();
                let addr_str = addr.to_string();

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req_text = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req_text.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return;
                    }
                    let method = parts[0];
                    let path = parts[1];

                    let (status, body) = if path == "/api/token" && method == "POST" {
                        auth_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        (
                            "200 OK",
                            r#"{"access_token":"mock_test_token","token_type":"Bearer","expires_in":3600}"#.to_string(),
                        )
                    } else if path.starts_with("/playlists/p123/tracks?limit=100&offset=0") {
                        let mut items = Vec::new();
                        for i in 1..=100 {
                            items.push(format!(
                                r#"{{"track":{{"id":"track_{i}","name":"Song {i}","artists":[{{"name":"Artist A"}}],"album":{{"name":"Album A"}},"duration_ms":180000,"track_number":{i},"external_ids":{{"isrc":"US12345{i:04}"}}}}}}"#
                            ));
                        }
                        let next_url = format!(
                            "http://{}/playlists/p123/tracks?limit=100&offset=100",
                            addr_str
                        );
                        let body =
                            format!(r#"{{"items":[{}],"next":"{}"}}"#, items.join(","), next_url);
                        ("200 OK", body)
                    } else if path.starts_with("/playlists/p123/tracks?limit=100&offset=100") {
                        let mut items = Vec::new();
                        for i in 101..=125 {
                            items.push(format!(
                                r#"{{"track":{{"id":"track_{i}","name":"Song {i}","artists":[{{"name":"Artist B"}}],"album":{{"name":"Album B"}},"duration_ms":200000,"track_number":{i},"external_ids":{{"isrc":"US12345{i:04}"}}}}}}"#
                            ));
                        }
                        let body = format!(r#"{{"items":[{}],"next":null}}"#, items.join(","));
                        ("200 OK", body)
                    } else if path == "/tracks/single_track_1" {
                        (
                            "200 OK",
                            r#"{"id":"single_track_1","name":"Single Hit","artists":[{"name":"Solo Artist"}],"album":{"name":"Solo Album"},"duration_ms":210000,"track_number":1,"external_ids":{"isrc":"GB1234567890"}}"#.to_string(),
                        )
                    } else if path == "/albums/album_1" {
                        (
                            "200 OK",
                            r#"{"id":"album_1","name":"Greatest Hits","tracks":{"items":[{"id":"hit_1","name":"Hit 1","artists":[{"name":"Band"}],"duration_ms":195000,"track_number":1,"external_ids":{"isrc":"DE1234567890"}}],"next":null}}"#.to_string(),
                        )
                    } else if path == "/tracks/non_existent" {
                        (
                            "404 Not Found",
                            r#"{"error":{"status":404,"message":"Not found"}}"#.to_string(),
                        )
                    } else {
                        ("404 Not Found", "Not Found".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });

        let client = SpotifyClient::with_base_urls(
            "test_client_id".to_string(),
            "test_client_secret".to_string(),
            base_url.clone(),
            token_url,
        );

        // 1. Authenticate and verify caching
        let token1 = client.authenticate().await.expect("auth 1 failed");
        assert_eq!(token1, "mock_test_token");
        assert_eq!(auth_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let token2 = client.authenticate().await.expect("auth 2 failed");
        assert_eq!(token2, "mock_test_token");
        // Count should still be 1 because it was cached!
        assert_eq!(auth_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        // 2. Fetch playlist with pagination (> 100 tracks)
        let tracks = client
            .fetch_playlist("p123")
            .await
            .expect("fetch playlist failed");
        assert_eq!(tracks.len(), 125);
        assert_eq!(tracks[0].id, "track_1");
        assert_eq!(tracks[0].title, "Song 1");
        assert_eq!(tracks[0].artists, vec!["Artist A".to_string()]);
        assert_eq!(tracks[0].album, "Album A");
        assert_eq!(tracks[0].duration_ms, 180000);
        assert_eq!(tracks[0].track_number, 1);
        assert_eq!(tracks[0].isrc, Some("US123450001".to_string()));

        assert_eq!(tracks[124].id, "track_125");
        assert_eq!(tracks[124].title, "Song 125");
        assert_eq!(tracks[124].artists, vec!["Artist B".to_string()]);
        assert_eq!(tracks[124].album, "Album B");
        assert_eq!(tracks[124].duration_ms, 200000);
        assert_eq!(tracks[124].track_number, 125);

        // 3. Fetch single track
        let track = client
            .fetch_track("single_track_1")
            .await
            .expect("fetch track failed");
        assert_eq!(track.id, "single_track_1");
        assert_eq!(track.title, "Single Hit");
        assert_eq!(track.artists, vec!["Solo Artist".to_string()]);
        assert_eq!(track.album, "Solo Album");
        assert_eq!(track.duration_ms, 210000);
        assert_eq!(track.isrc, Some("GB1234567890".to_string()));

        // 4. Fetch album
        let album_tracks = client
            .fetch_album("album_1")
            .await
            .expect("fetch album failed");
        assert_eq!(album_tracks.len(), 1);
        assert_eq!(album_tracks[0].id, "hit_1");
        assert_eq!(album_tracks[0].album, "Greatest Hits");

        // 5. Fetch via URL string dispatcher
        let url_tracks = client
            .fetch("https://open.spotify.com/playlist/p123")
            .await
            .unwrap();
        assert_eq!(url_tracks.len(), 125);

        let single_url_tracks = client
            .fetch("https://open.spotify.com/track/single_track_1")
            .await
            .unwrap();
        assert_eq!(single_url_tracks.len(), 1);
        assert_eq!(single_url_tracks[0].id, "single_track_1");

        // 6. Not found error
        let not_found_err = client.fetch_track("non_existent").await;
        assert!(matches!(not_found_err, Err(SpotifyError::NotFound(_))));
    }

    #[test]
    fn test_oauth_dtos_serialization() {
        let playlist = SpotifyPlaylistSummary {
            id: "pl_test_123".to_string(),
            name: "Summer Vibes".to_string(),
            description: Some("Chill tracks for the sun".to_string()),
            track_count: 35,
            image_url: Some("https://example.com/cover.jpg".to_string()),
        };

        let json = serde_json::to_string(&playlist).expect("serialize playlist");
        let deserialized: SpotifyPlaylistSummary =
            serde_json::from_str(&json).expect("deserialize playlist");
        assert_eq!(playlist, deserialized);

        let profile = UserProfile {
            display_name: "John Doe".to_string(),
            id: "johndoe123".to_string(),
            image_url: Some("https://example.com/profile.jpg".to_string()),
        };

        let p_json = serde_json::to_string(&profile).expect("serialize profile");
        let p_deserialized: UserProfile =
            serde_json::from_str(&p_json).expect("deserialize profile");
        assert_eq!(profile, p_deserialized);

        let tokens = OAuthTokens {
            access_token: "at_123".to_string(),
            refresh_token: "rt_456".to_string(),
            expires_in: 3600,
        };
        let t_json = serde_json::to_string(&tokens).expect("serialize tokens");
        let t_deserialized: OAuthTokens =
            serde_json::from_str(&t_json).expect("deserialize tokens");
        assert_eq!(tokens, t_deserialized);
    }

    #[test]
    fn test_query_param_extraction_and_url_encode() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(
            url_encode("http://127.0.0.1:8888/callback"),
            "http%3A%2F%2F127.0.0.1%3A8888%2Fcallback"
        );

        let req = "GET /callback?code=mock_code_xyz&state=random_state HTTP/1.1\r\nHost: 127.0.0.1:8888\r\n\r\n";
        assert_eq!(
            extract_query_param(req, "code").as_deref(),
            Some("mock_code_xyz")
        );
        assert_eq!(
            extract_query_param(req, "state").as_deref(),
            Some("random_state")
        );
        assert_eq!(extract_query_param(req, "non_existent"), None);

        let err_req = "GET /callback?error=access_denied HTTP/1.1\r\n\r\n";
        assert_eq!(
            extract_query_param(err_req, "error").as_deref(),
            Some("access_denied")
        );
        assert_eq!(extract_query_param(err_req, "code"), None);
    }

    #[tokio::test]
    async fn test_oauth_loopback_flow() {
        // 1. Mock token exchange server
        let token_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token_addr = token_listener.local_addr().unwrap();
        let token_url = format!("http://{}/api/token", token_addr);

        tokio::spawn(async move {
            let (mut socket, _) = token_listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let req_text = String::from_utf8_lossy(&buf[..n]);

            assert!(req_text.contains("grant_type=authorization_code"));
            assert!(req_text.contains("code=test_auth_code_789"));

            let body = r#"{"access_token":"oauth_token_success","token_type":"Bearer","expires_in":3600,"refresh_token":"oauth_refresh_token_success"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        });

        // 2. Loopback listener on random free port
        let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let loopback_port = loopback_listener.local_addr().unwrap().port();

        let loopback_task = tokio::spawn(async move {
            start_oauth_loopback_with_listener(
                loopback_listener,
                "client_id_test",
                "client_secret_test",
                "http://fake_auth",
                &token_url,
                false, // do not spawn browser
            )
            .await
        });

        // 3. Client browser callback simulation
        let callback_url = format!(
            "http://127.0.0.1:{}/callback?code=test_auth_code_789",
            loopback_port
        );
        let resp = reqwest::get(&callback_url)
            .await
            .expect("simulate callback");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body = resp.text().await.unwrap();
        assert!(body.contains("Anmeldung bei SpotyBurn erfolgreich!"));

        // 4. Verify token exchange result
        let tokens = loopback_task.await.unwrap().expect("OAuth loopback failed");
        assert_eq!(tokens.access_token, "oauth_token_success");
        assert_eq!(tokens.refresh_token, "oauth_refresh_token_success");
        assert_eq!(tokens.expires_in, 3600);
    }

    #[tokio::test]
    async fn test_refresh_token_and_user_profile_and_playlists() {
        let server_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = server_listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let token_url = format!("http://{}/api/token", addr);

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match server_listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                let addr_str = addr.to_string();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req_text = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req_text.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return;
                    }
                    let method = parts[0];
                    let path = parts[1];

                    let (status, body) = if path == "/api/token" && method == "POST" {
                        assert!(req_text.contains("grant_type=refresh_token"));
                        (
                            "200 OK",
                            r#"{"access_token":"refreshed_token_123","token_type":"Bearer","expires_in":3600,"refresh_token":"new_refresh_token_456"}"#.to_string(),
                        )
                    } else if path == "/me" && method == "GET" {
                        (
                            "200 OK",
                            r#"{"id":"user_test_42","display_name":"SpotyMaster","images":[{"url":"https://example.com/avatar.png","height":300,"width":300}]}"#.to_string(),
                        )
                    } else if path.starts_with("/me/playlists?limit=50") && !path.contains("offset")
                    {
                        let next_url =
                            format!("http://{}/me/playlists?limit=50&offset=50", addr_str);
                        let body = format!(
                            r#"{{"items":[{{"id":"pl_1","name":"Roadtrip","description":"Fun tunes","tracks":{{"total":25}},"images":[{{"url":"https://example.com/c1.jpg"}}]}}],"next":"{}"}}"#,
                            next_url
                        );
                        ("200 OK", body)
                    } else if path.contains("offset=50") {
                        (
                            "200 OK",
                            r#"{"items":[{"id":"pl_2","name":"Focus","description":"","tracks":{"total":50},"images":[]}],"next":null}"#.to_string(),
                        )
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });

        // 1. Direct refresh_access_token call
        let client_req = reqwest::Client::new();
        let tokens = refresh_access_token_internal(
            &client_req,
            &token_url,
            "cid",
            "csec",
            "initial_refresh_token",
        )
        .await
        .expect("refresh_access_token failed");

        assert_eq!(tokens.access_token, "refreshed_token_123");
        assert_eq!(tokens.refresh_token, "new_refresh_token_456");

        // 2. SpotifyClient with refresh token
        let client = SpotifyClient::with_base_urls(
            "cid".to_string(),
            "csec".to_string(),
            base_url.clone(),
            token_url.clone(),
        );
        client
            .set_refresh_token(Some("initial_refresh_token".to_string()))
            .await;

        // Verify profile fetch
        let profile = client
            .fetch_current_user_profile()
            .await
            .expect("fetch_current_user_profile failed");
        assert_eq!(profile.id, "user_test_42");
        assert_eq!(profile.display_name, "SpotyMaster");
        assert_eq!(
            profile.image_url.as_deref(),
            Some("https://example.com/avatar.png")
        );

        // Verify playlists fetch with pagination
        let playlists = client
            .fetch_user_playlists()
            .await
            .expect("fetch_user_playlists failed");
        assert_eq!(playlists.len(), 2);
        assert_eq!(playlists[0].id, "pl_1");
        assert_eq!(playlists[0].name, "Roadtrip");
        assert_eq!(playlists[0].description.as_deref(), Some("Fun tunes"));
        assert_eq!(playlists[0].track_count, 25);
        assert_eq!(
            playlists[0].image_url.as_deref(),
            Some("https://example.com/c1.jpg")
        );

        assert_eq!(playlists[1].id, "pl_2");
        assert_eq!(playlists[1].name, "Focus");
        assert_eq!(playlists[1].description, None);
        assert_eq!(playlists[1].track_count, 50);
        assert_eq!(playlists[1].image_url, None);
    }

    #[test]
    fn test_build_search_url_encoding() {
        let base_url = "https://api.spotify.com/v1";

        // 1. Standard query with spaces and ampersand
        let url1 = build_search_url(base_url, "Rock & Roll", &["track"], 10);
        assert!(url1.contains("q=Rock+%26+Roll") || url1.contains("q=Rock%20%26%20Roll"));
        assert!(url1.contains("type=track"));
        assert!(url1.contains("limit=10"));

        // 2. Special characters, slashes, umlauts
        let url2 = build_search_url(
            base_url,
            "AC/DC - Über Hits 100%",
            &["track", "album"],
            0, // Should default to 10
        );
        assert!(url2.contains("type=track%2Calbum"));
        assert!(url2.contains("limit=10"));
        // URL should parse cleanly
        let parsed = reqwest::Url::parse(&url2).expect("valid URL");
        let q_val = parsed.query_pairs().find(|(k, _)| k == "q").unwrap().1;
        assert_eq!(q_val, "AC/DC - Über Hits 100%");

        // 3. Empty types should default to track,playlist,album
        let url3 = build_search_url(base_url, "Queen", &[], 100);
        assert!(url3.contains("type=track%2Cplaylist%2Calbum"));
        assert!(url3.contains("limit=10")); // Clamped to 10
    }

    #[test]
    fn test_parse_search_response_full() {
        let json = r#"{
            "tracks": {
                "items": [
                    {
                        "id": "trk_1",
                        "name": "Bohemian Rhapsody",
                        "artists": [{"name": "Queen"}],
                        "album": {"name": "A Night at the Opera"},
                        "duration_ms": 354320,
                        "track_number": 11,
                        "external_ids": {"isrc": "GBUM71029604"}
                    }
                ]
            },
            "playlists": {
                "items": [
                    {
                        "id": "pl_1",
                        "name": "70s Rock Anthems",
                        "description": "Best classic rock",
                        "images": [{"url": "https://example.com/cover.jpg"}],
                        "tracks": {"total": 85}
                    }
                ]
            },
            "albums": {
                "items": [
                    {
                        "id": "alb_1",
                        "name": "A Night at the Opera",
                        "artists": [{"name": "Queen"}],
                        "total_tracks": 12,
                        "release_date": "1975-11-21",
                        "images": [{"url": "https://example.com/album.jpg"}]
                    }
                ]
            }
        }"#;

        let res = parse_search_response(json).expect("parse search response");
        assert_eq!(res.tracks.len(), 1);
        assert_eq!(res.tracks[0].id, "trk_1");
        assert_eq!(res.tracks[0].title, "Bohemian Rhapsody");
        assert_eq!(res.tracks[0].artists, vec!["Queen".to_string()]);
        assert_eq!(res.tracks[0].album, "A Night at the Opera");
        assert_eq!(res.tracks[0].duration_ms, 354320);
        assert_eq!(res.tracks[0].track_number, 11);
        assert_eq!(res.tracks[0].isrc.as_deref(), Some("GBUM71029604"));

        assert_eq!(res.playlists.len(), 1);
        assert_eq!(res.playlists[0].id, "pl_1");
        assert_eq!(res.playlists[0].name, "70s Rock Anthems");
        assert_eq!(
            res.playlists[0].description.as_deref(),
            Some("Best classic rock")
        );
        assert_eq!(res.playlists[0].track_count, 85);
        assert_eq!(
            res.playlists[0].image_url.as_deref(),
            Some("https://example.com/cover.jpg")
        );

        assert_eq!(res.albums.len(), 1);
        assert_eq!(res.albums[0].id, "alb_1");
        assert_eq!(res.albums[0].name, "A Night at the Opera");
        assert_eq!(res.albums[0].artists, vec!["Queen".to_string()]);
        assert_eq!(res.albums[0].total_tracks, 12);
        assert_eq!(res.albums[0].release_date.as_deref(), Some("1975-11-21"));
        assert_eq!(
            res.albums[0].image_url.as_deref(),
            Some("https://example.com/album.jpg")
        );
    }

    #[test]
    fn test_parse_search_response_nulls_and_empty() {
        // Empty JSON should yield empty result without error
        let empty_res = parse_search_response("{}").expect("parse empty");
        assert!(empty_res.tracks.is_empty());
        assert!(empty_res.playlists.is_empty());
        assert!(empty_res.albums.is_empty());

        // Spotify null items in array (deleted playlists/tracks)
        let json_with_nulls = r#"{
            "tracks": {
                "items": [null, {
                    "id": "t2",
                    "name": "Live Song",
                    "artists": [{"name": "Artist"}],
                    "album": null,
                    "duration_ms": 120000,
                    "track_number": 1,
                    "external_ids": null
                }]
            },
            "playlists": {
                "items": [null, {
                    "id": "p2",
                    "name": "Only Playlist",
                    "description": "",
                    "images": [],
                    "tracks": null
                }]
            },
            "albums": {
                "items": [null]
            }
        }"#;

        let res = parse_search_response(json_with_nulls).expect("parse null items");
        assert_eq!(res.tracks.len(), 1);
        assert_eq!(res.tracks[0].id, "t2");
        assert_eq!(res.tracks[0].album, "");
        assert_eq!(res.tracks[0].isrc, None);

        assert_eq!(res.playlists.len(), 1);
        assert_eq!(res.playlists[0].id, "p2");
        assert_eq!(res.playlists[0].description, None);
        assert_eq!(res.playlists[0].image_url, None);
        assert_eq!(res.playlists[0].track_count, 0);

        assert!(res.albums.is_empty());
    }

    #[tokio::test]
    async fn test_search_empty_query_returns_default() {
        let client = SpotifyClient::new("id".into(), "sec".into());
        let res = client.search("   ", &[], 20).await.expect("empty query");
        assert_eq!(res, SpotifySearchResult::default());
    }

    #[tokio::test]
    async fn test_search_dual_token_client_credentials() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let token_url = format!("http://{}/api/token", addr);

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req_text = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req_text.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return;
                    }
                    let method = parts[0];
                    let path = parts[1];

                    let (status, body) = if path == "/api/token" && method == "POST" {
                        (
                            "200 OK",
                            r#"{"access_token":"client_cred_token","token_type":"Bearer","expires_in":3600}"#.to_string(),
                        )
                    } else if path.starts_with("/search") {
                        // Check that Bearer client_cred_token was sent
                        assert!(
                            req_text.contains("authorization: Bearer client_cred_token")
                                || req_text.contains("Authorization: Bearer client_cred_token")
                        );
                        (
                            "200 OK",
                            r#"{"tracks":{"items":[{"id":"trk_cc","name":"Client Cred Track","artists":[{"name":"Band"}],"album":{"name":"Album"},"duration_ms":100000,"track_number":1,"external_ids":null}]}}"#.to_string(),
                        )
                    } else {
                        ("404 Not Found", "Not Found".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });

        let client = SpotifyClient::with_base_urls(
            "test_client_id".to_string(),
            "test_client_secret".to_string(),
            base_url,
            token_url,
        );

        let result = client
            .search("Client Cred Track", &["track"], 10)
            .await
            .expect("search succeeded");

        assert_eq!(result.tracks.len(), 1);
        assert_eq!(result.tracks[0].id, "trk_cc");
        assert_eq!(result.tracks[0].title, "Client Cred Track");
    }

    #[tokio::test]
    async fn test_search_dual_token_user_access_token_and_fallback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        let token_url = format!("http://{}/api/token", addr);

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req_text = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req_text.lines().next().unwrap_or("");
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return;
                    }
                    let method = parts[0];
                    let path = parts[1];

                    let (status, body) = if path == "/api/token" && method == "POST" {
                        (
                            "200 OK",
                            r#"{"access_token":"fallback_client_token","token_type":"Bearer","expires_in":3600}"#.to_string(),
                        )
                    } else if path.starts_with("/search") {
                        if req_text.contains("Bearer valid_user_token") {
                            (
                                "200 OK",
                                r#"{"tracks":{"items":[{"id":"trk_user","name":"User Track","artists":[{"name":"Solo"}],"album":{"name":"Album"},"duration_ms":200000,"track_number":1,"external_ids":null}]}}"#.to_string(),
                            )
                        } else if req_text.contains("Bearer expired_user_token") {
                            // Returns 401 to trigger Dual-Token Fallback!
                            (
                                "401 Unauthorized",
                                r#"{"error":{"status":401,"message":"The access token expired"}}"#
                                    .to_string(),
                            )
                        } else if req_text.contains("Bearer fallback_client_token") {
                            (
                                "200 OK",
                                r#"{"tracks":{"items":[{"id":"trk_fallback","name":"Fallback Track","artists":[{"name":"Fallback"}],"album":{"name":"Album"},"duration_ms":250000,"track_number":1,"external_ids":null}]}}"#.to_string(),
                            )
                        } else {
                            ("403 Forbidden", "Forbidden".to_string())
                        }
                    } else {
                        ("404 Not Found", "Not Found".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });

        // 1. Client with valid user token -> uses user token directly
        let client_user = SpotifyClient::with_base_urls(
            "cid".to_string(),
            "csec".to_string(),
            base_url.clone(),
            token_url.clone(),
        );
        client_user
            .set_user_access_token(Some("valid_user_token".to_string()))
            .await;

        let res1 = client_user
            .search("User Track", &["track"], 5)
            .await
            .expect("user search succeeded");
        assert_eq!(res1.tracks.len(), 1);
        assert_eq!(res1.tracks[0].id, "trk_user");

        // 2. Client with expired user token -> falls back to client credentials on 401
        let client_fallback = SpotifyClient::with_base_urls(
            "cid".to_string(),
            "csec".to_string(),
            base_url.clone(),
            token_url.clone(),
        );
        client_fallback
            .set_user_access_token(Some("expired_user_token".to_string()))
            .await;

        let res2 = client_fallback
            .search("Fallback Track", &["track"], 5)
            .await
            .expect("fallback search succeeded");
        assert_eq!(res2.tracks.len(), 1);
        assert_eq!(res2.tracks[0].id, "trk_fallback");
    }

    #[test]
    fn test_extract_next_data_json() {
        let html = r#"<html><head><script id="__NEXT_DATA__" type="application/json">{"props":{"test":123}}</script></head><body></body></html>"#;
        let json = extract_next_data_json(html);
        assert_eq!(json, Some(r#"{"props":{"test":123}}"#));

        let missing = "<html><body>no script</body></html>";
        assert_eq!(extract_next_data_json(missing), None);
    }

    #[test]
    fn test_parse_embed_tracks_playlist() {
        let sample_html = r#"
        <!DOCTYPE html>
        <html>
        <head>
          <script id="__NEXT_DATA__" type="application/json">
          {
            "props": {
              "pageProps": {
                "state": {
                  "data": {
                    "entity": {
                      "type": "playlist",
                      "name": "Cool 80s Hits",
                      "trackList": [
                        {
                          "uri": "spotify:track:4cOdK2wGLETKBW3PvgPWqT",
                          "title": "Never Gonna Give You Up",
                          "subtitle": "Rick Astley",
                          "duration": 213573
                        },
                        {
                          "uri": "spotify:track:11dFghVXANMlKmJXsNCbNl",
                          "title": "Take On Me",
                          "subtitle": "a-ha,\u00a0Magne Furuholmen",
                          "duration": 225280
                        }
                      ]
                    }
                  }
                }
              }
            }
          }
          </script>
        </head>
        <body></body>
        </html>
        "#;

        let tracks = parse_embed_tracks(sample_html).expect("parsed embed tracks");
        assert_eq!(tracks.len(), 2);

        assert_eq!(tracks[0].id, "4cOdK2wGLETKBW3PvgPWqT");
        assert_eq!(tracks[0].title, "Never Gonna Give You Up");
        assert_eq!(tracks[0].artists, vec!["Rick Astley"]);
        assert_eq!(tracks[0].album, "Cool 80s Hits");
        assert_eq!(tracks[0].duration_ms, 213573);
        assert_eq!(tracks[0].track_number, 1);

        assert_eq!(tracks[1].id, "11dFghVXANMlKmJXsNCbNl");
        assert_eq!(tracks[1].title, "Take On Me");
        assert_eq!(tracks[1].artists, vec!["a-ha", "Magne Furuholmen"]);
        assert_eq!(tracks[1].album, "Cool 80s Hits");
        assert_eq!(tracks[1].duration_ms, 225280);
        assert_eq!(tracks[1].track_number, 2);
    }

    #[test]
    fn test_parse_embed_tracks_single_track() {
        let sample_html = r#"
        <!DOCTYPE html>
        <html>
        <head>
          <script id="__NEXT_DATA__" type="application/json">
          {
            "props": {
              "pageProps": {
                "state": {
                  "data": {
                    "entity": {
                      "type": "track",
                      "id": "3h5T5JypYU7huFiVYhv1dr",
                      "title": "BbY WOW",
                      "artists": [
                        { "name": "KAROL G" },
                        { "name": "Judeline" }
                      ],
                      "duration": 225834
                    }
                  }
                }
              }
            }
          }
          </script>
        </head>
        <body></body>
        </html>
        "#;

        let tracks = parse_embed_tracks(sample_html).expect("parsed embed track");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "3h5T5JypYU7huFiVYhv1dr");
        assert_eq!(tracks[0].title, "BbY WOW");
        assert_eq!(tracks[0].artists, vec!["KAROL G", "Judeline"]);
        assert_eq!(tracks[0].duration_ms, 225834);
    }
}
