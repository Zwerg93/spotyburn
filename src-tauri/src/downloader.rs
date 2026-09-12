use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::SpotifyTrack;

/// Default tolerance in milliseconds (±5 seconds)
pub const DEFAULT_DURATION_TOLERANCE_MS: u64 = 5_000;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("yt-dlp binary not found in PATH or ~/.spotyburn/bin")]
    YtDlpNotFound,

    #[error("Failed to execute yt-dlp: {0}")]
    ExecutionFailed(String),

    #[error("No candidate video matched the duration tolerance for track '{title}' (expected: {expected_ms}ms, candidates checked: {candidates_checked})")]
    NoMatchingCandidate {
        title: String,
        expected_ms: u64,
        candidates_checked: usize,
    },

    #[error("Invalid or empty response from yt-dlp search: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Information about a candidate retrieved from yt-dlp search
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct YtCandidate {
    pub id: String,
    pub title: Option<String>,
    /// Duration in seconds (as returned by yt-dlp)
    pub duration: Option<f64>,
    pub webpage_url: Option<String>,
}

impl YtCandidate {
    /// Returns the candidate duration in milliseconds if available.
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration.map(|d| (d * 1000.0).round() as u64)
    }
}

/// Generates a standardized search query for a track:
/// `"{artist} - {title} (Official Audio)"`
pub fn build_search_query(artist: &str, title: &str) -> String {
    format!("{} - {} (Official Audio)", artist.trim(), title.trim())
}

/// Generates a standardized search query for a `SpotifyTrack`.
pub fn build_search_query_for_track(track: &SpotifyTrack) -> String {
    let artist_str = if track.artists.is_empty() {
        "Unknown Artist".to_string()
    } else {
        track.artists.join(", ")
    };
    build_search_query(&artist_str, &track.title)
}

/// Checks whether a candidate's duration is within tolerance (default ±5 seconds)
/// of the expected Spotify track duration.
pub fn is_duration_within_tolerance(
    spotify_duration_ms: u64,
    candidate_duration_ms: u64,
    tolerance_ms: u64,
) -> bool {
    let diff = (spotify_duration_ms as i64 - candidate_duration_ms as i64).abs();
    diff <= tolerance_ms as i64
}

/// Checks whether a candidate's duration matches the Spotify duration within ±5 seconds.
pub fn check_duration_tolerance(spotify_duration_ms: u64, candidate_duration_ms: u64) -> bool {
    is_duration_within_tolerance(
        spotify_duration_ms,
        candidate_duration_ms,
        DEFAULT_DURATION_TOLERANCE_MS,
    )
}

/// Selects the best candidate matching the duration tolerance (checked in order of relevance).
pub fn select_best_candidate(
    candidates: &[YtCandidate],
    expected_duration_ms: u64,
    tolerance_ms: u64,
) -> Option<&YtCandidate> {
    for candidate in candidates {
        if let Some(cand_ms) = candidate.duration_ms() {
            if is_duration_within_tolerance(expected_duration_ms, cand_ms, tolerance_ms) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Locates the `yt-dlp` executable on the system:
/// 1. Optional explicit override / `SPOTYBURN_YT_DLP_PATH` environment variable.
/// 2. `~/.spotyburn/bin/yt-dlp` (or `.exe` on Windows).
/// 3. In the system `PATH` or standard directories.
pub fn find_yt_dlp() -> Result<PathBuf, DownloadError> {
    find_yt_dlp_with_hint(None)
}

/// Locates `yt-dlp` with an optional custom path hint.
pub fn find_yt_dlp_with_hint(hint: Option<&Path>) -> Result<PathBuf, DownloadError> {
    if let Some(custom) = hint {
        if custom.is_file() {
            return Ok(custom.to_path_buf());
        }
    }

    if let Ok(env_path) = std::env::var("SPOTYBURN_YT_DLP_PATH") {
        let p = PathBuf::from(env_path);
        if p.is_file() {
            return Ok(p);
        }
    }

    let bin_name = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };

    // Check ~/.spotyburn/bin/yt-dlp
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".spotyburn").join("bin").join(bin_name);
        if local_bin.is_file() {
            return Ok(local_bin);
        }
    }

    // Check common Unix paths
    if !cfg!(windows) {
        let common_paths = [
            Path::new("/opt/homebrew/bin/yt-dlp"),
            Path::new("/usr/local/bin/yt-dlp"),
            Path::new("/usr/bin/yt-dlp"),
        ];
        for path in &common_paths {
            if path.is_file() {
                return Ok(path.to_path_buf());
            }
        }
    }

    // Scan PATH environment variable
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(bin_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(DownloadError::YtDlpNotFound)
}

/// Downloader orchestrator using `yt-dlp`
#[derive(Debug, Clone)]
pub struct YtDlpDownloader {
    pub binary_path: PathBuf,
    pub tolerance_ms: u64,
}

impl YtDlpDownloader {
    pub fn new() -> Result<Self, DownloadError> {
        let binary_path = find_yt_dlp()?;
        Ok(Self {
            binary_path,
            tolerance_ms: DEFAULT_DURATION_TOLERANCE_MS,
        })
    }

    pub fn with_binary(binary_path: PathBuf) -> Self {
        Self {
            binary_path,
            tolerance_ms: DEFAULT_DURATION_TOLERANCE_MS,
        }
    }

    pub fn with_tolerance(mut self, tolerance_ms: u64) -> Self {
        self.tolerance_ms = tolerance_ms;
        self
    }

    /// Searches for candidates using yt-dlp metadata JSON extraction without downloading.
    pub fn search_candidates(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<YtCandidate>, DownloadError> {
        let search_spec = format!("ytsearch{}:{}", limit, query);
        let output = Command::new(&self.binary_path)
            .arg("--dump-json")
            .arg("--no-playlist")
            .arg("--default-search")
            .arg("auto")
            .arg(&search_spec)
            .output()
            .map_err(|e| DownloadError::ExecutionFailed(e.to_string()))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(DownloadError::ExecutionFailed(err.into_owned()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_candidates_from_json_stream(&stdout)
    }

    /// Orchestrates matching and downloading a track:
    /// 1. Builds search query `"{artist} - {title} (Official Audio)"`
    /// 2. Searches candidates and verifies duration tolerance (±5s)
    /// 3. Downloads the matched candidate audio into `cache_dir`
    /// 4. Returns the path of the downloaded file
    pub fn match_and_download(
        &self,
        track: &SpotifyTrack,
        cache_dir: &Path,
    ) -> Result<PathBuf, DownloadError> {
        std::fs::create_dir_all(cache_dir)?;

        let query = build_search_query_for_track(track);
        let candidates = self.search_candidates(&query, 5)?;

        let candidate = select_best_candidate(&candidates, track.duration_ms, self.tolerance_ms)
            .ok_or_else(|| DownloadError::NoMatchingCandidate {
                title: track.title.clone(),
                expected_ms: track.duration_ms,
                candidates_checked: candidates.len(),
            })?;

        let url = candidate
            .webpage_url
            .clone()
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", candidate.id));

        // Output template using track ID
        let output_template = cache_dir.join(format!("{}.%(ext)s", track.id));

        let download_output = Command::new(&self.binary_path)
            .arg("-f")
            .arg("ba/b")
            .arg("--no-playlist")
            .arg("-o")
            .arg(output_template.to_string_lossy().as_ref())
            .arg("--print")
            .arg("after_move:filepath")
            .arg(&url)
            .output()
            .map_err(|e| DownloadError::ExecutionFailed(e.to_string()))?;

        if !download_output.status.success() {
            let err = String::from_utf8_lossy(&download_output.stderr);
            return Err(DownloadError::ExecutionFailed(err.into_owned()));
        }

        let printed_path = String::from_utf8_lossy(&download_output.stdout)
            .trim()
            .to_string();
        if !printed_path.is_empty() {
            let p = PathBuf::from(printed_path);
            if p.is_file() {
                return Ok(p);
            }
        }

        // Fallback check: find any file in cache_dir starting with track.id
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(stem) = path.file_stem() {
                    if stem == track.id.as_str() && path.is_file() {
                        return Ok(path);
                    }
                }
            }
        }

        Err(DownloadError::ExecutionFailed(
            "Download succeeded but downloaded file could not be located".to_string(),
        ))
    }
}

/// Parses newline-delimited JSON stream output from yt-dlp `--dump-json`
pub fn parse_candidates_from_json_stream(
    json_stream: &str,
) -> Result<Vec<YtCandidate>, DownloadError> {
    let mut candidates = Vec::new();
    for line in json_stream.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(cand) = serde_json::from_str::<YtCandidate>(trimmed) {
            candidates.push(cand);
        }
    }
    if candidates.is_empty() && !json_stream.trim().is_empty() {
        return Err(DownloadError::ParseError(
            "Could not parse JSON candidate output".to_string(),
        ));
    }
    Ok(candidates)
}

/// Convenience standalone function matching spec 3.2
pub fn match_and_download(
    track: &SpotifyTrack,
    cache_dir: &Path,
) -> Result<PathBuf, DownloadError> {
    let downloader = YtDlpDownloader::new()?;
    downloader.match_and_download(track, cache_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_search_query() {
        let query = build_search_query("Daft Punk", "Get Lucky");
        assert_eq!(query, "Daft Punk - Get Lucky (Official Audio)");

        let query_trimmed = build_search_query("  The Weeknd  ", "  Blinding Lights ");
        assert_eq!(
            query_trimmed,
            "The Weeknd - Blinding Lights (Official Audio)"
        );
    }

    #[test]
    fn test_build_search_query_for_track() {
        let track = SpotifyTrack {
            id: "track123".to_string(),
            title: "Harder, Better, Faster, Stronger".to_string(),
            artists: vec!["Daft Punk".to_string()],
            album: "Discovery".to_string(),
            duration_ms: 224000,
            track_number: 4,
            isrc: Some("FRZ030100440".to_string()),
        };

        assert_eq!(
            build_search_query_for_track(&track),
            "Daft Punk - Harder, Better, Faster, Stronger (Official Audio)"
        );

        let multi_artist_track = SpotifyTrack {
            id: "track456".to_string(),
            title: "Starboy".to_string(),
            artists: vec!["The Weeknd".to_string(), "Daft Punk".to_string()],
            album: "Starboy".to_string(),
            duration_ms: 230000,
            track_number: 1,
            isrc: None,
        };

        assert_eq!(
            build_search_query_for_track(&multi_artist_track),
            "The Weeknd, Daft Punk - Starboy (Official Audio)"
        );
    }

    #[test]
    fn test_duration_tolerance() {
        let spotify_ms = 200_000; // 200 seconds

        // Exact match
        assert!(check_duration_tolerance(spotify_ms, 200_000));

        // Within +5 seconds (5000 ms)
        assert!(check_duration_tolerance(spotify_ms, 205_000));
        // Within -5 seconds
        assert!(check_duration_tolerance(spotify_ms, 195_000));

        // 1 ms beyond boundary (+5001 ms) -> false
        assert!(!check_duration_tolerance(spotify_ms, 205_001));
        // 1 ms beyond boundary (-5001 ms) -> false
        assert!(!check_duration_tolerance(spotify_ms, 194_999));

        // Huge difference -> false
        assert!(!check_duration_tolerance(spotify_ms, 300_000));
        assert!(!check_duration_tolerance(spotify_ms, 50_000));
    }

    #[test]
    fn test_select_best_candidate() {
        let candidates = vec![
            YtCandidate {
                id: "too_long".to_string(),
                title: Some("Extended Version".to_string()),
                duration: Some(300.0), // 300s = 300_000ms (+100s)
                webpage_url: None,
            },
            YtCandidate {
                id: "too_short".to_string(),
                title: Some("Preview".to_string()),
                duration: Some(30.0), // 30s (-170s)
                webpage_url: None,
            },
            YtCandidate {
                id: "perfect_match".to_string(),
                title: Some("Official Audio".to_string()),
                duration: Some(202.5), // 202.5s = 202_500ms (+2.5s -> within 5s)
                webpage_url: Some("https://youtube.com/watch?v=perfect_match".to_string()),
            },
            YtCandidate {
                id: "also_valid_but_second".to_string(),
                title: Some("Alternative".to_string()),
                duration: Some(200.0),
                webpage_url: None,
            },
        ];

        let selected = select_best_candidate(&candidates, 200_000, 5_000);
        assert!(selected.is_some());
        let matched = selected.unwrap();
        assert_eq!(matched.id, "perfect_match");
    }

    #[test]
    fn test_select_best_candidate_none_matched() {
        let candidates = vec![
            YtCandidate {
                id: "out1".to_string(),
                title: Some("Live".to_string()),
                duration: Some(215.0), // 15s difference
                webpage_url: None,
            },
            YtCandidate {
                id: "out2".to_string(),
                title: Some("Remix".to_string()),
                duration: Some(180.0), // 20s difference
                webpage_url: None,
            },
        ];

        let selected = select_best_candidate(&candidates, 200_000, 5_000);
        assert!(selected.is_none());
    }

    #[test]
    fn test_parse_candidates_from_json_stream() {
        let json_line_1 =
            r#"{"id":"vid1","title":"Song 1","duration":180.5,"webpage_url":"https://yt.com/1"}"#;
        let json_line_2 =
            r#"{"id":"vid2","title":"Song 2","duration":210.0,"webpage_url":"https://yt.com/2"}"#;
        let stream = format!("{}\n{}\n", json_line_1, json_line_2);

        let candidates = parse_candidates_from_json_stream(&stream).expect("Parsing candidates");
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, "vid1");
        assert_eq!(candidates[0].duration_ms(), Some(180500));
        assert_eq!(candidates[1].id, "vid2");
        assert_eq!(candidates[1].duration_ms(), Some(210000));
    }

    #[test]
    fn test_find_yt_dlp_with_hint() {
        let temp_dir = std::env::temp_dir();
        let dummy_bin = temp_dir.join("dummy_ytdlp_test_bin");
        std::fs::write(&dummy_bin, b"test").expect("Failed to write dummy binary");

        let found = find_yt_dlp_with_hint(Some(&dummy_bin));
        assert!(found.is_ok());
        assert_eq!(found.unwrap(), dummy_bin);

        let _ = std::fs::remove_file(dummy_bin);
    }
}
