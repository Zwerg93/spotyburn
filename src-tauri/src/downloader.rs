use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::SpotifyTrack;

/// Default tolerance in milliseconds (±15 seconds) — generous enough for YouTube versions
/// that include intros, outros, or slightly different edits vs. Spotify metadata
pub const DEFAULT_DURATION_TOLERANCE_MS: u64 = 15_000;

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

/// Returns an augmented `PATH` environment variable containing:
/// `~/.spotyburn/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, `/bin`,
/// plus existing system PATH entries, eliminating duplicate entries while preserving order.
pub fn get_augmented_path() -> OsString {
    let mut entries: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut add_entry = |p: PathBuf| {
        if seen.insert(p.clone()) {
            entries.push(p);
        }
    };

    if let Some(home) = dirs::home_dir() {
        add_entry(home.join(".spotyburn").join("bin"));
    }
    add_entry(PathBuf::from("/opt/homebrew/bin"));
    add_entry(PathBuf::from("/usr/local/bin"));
    add_entry(PathBuf::from("/usr/bin"));
    add_entry(PathBuf::from("/bin"));

    if let Some(system_path) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&system_path) {
            add_entry(p);
        }
    }

    std::env::join_paths(entries).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

/// Detects available JavaScript runtimes for `yt-dlp` signature decryption (n-sig).
/// If `deno` is found in the augmented PATH, returns an empty vector (yt-dlp uses Deno automatically).
/// If `deno` is not found, but `node` is present, returns `vec!["--js-runtimes", format!("node:{}", node_path.display())]`.
/// If neither is found, returns an empty vector.
pub fn detect_js_runtime_args() -> Vec<String> {
    let aug_path = get_augmented_path();
    let dirs: Vec<PathBuf> = std::env::split_paths(&aug_path).collect();

    let deno_name = if cfg!(windows) { "deno.exe" } else { "deno" };
    for dir in &dirs {
        let candidate = dir.join(deno_name);
        if candidate.is_file() {
            return Vec::new();
        }
    }

    let node_name = if cfg!(windows) { "node.exe" } else { "node" };
    for dir in &dirs {
        let candidate = dir.join(node_name);
        if candidate.is_file() {
            return vec![
                "--js-runtimes".to_string(),
                format!("node:{}", candidate.display()),
            ];
        }
    }

    Vec::new()
}

/// Ranks YouTube candidates for downloading:
/// - Group 1: Candidates whose duration is within `tolerance_ms`, sorted by lowest duration difference `|expected - actual|`.
/// - Group 2: Remaining candidates, sorted by lowest duration difference if known, or by original relevance order.
pub fn rank_candidates<'a>(
    candidates: &'a [YtCandidate],
    expected_duration_ms: u64,
    tolerance_ms: u64,
) -> Vec<&'a YtCandidate> {
    let mut group1: Vec<(usize, &'a YtCandidate, u64)> = Vec::new();
    let mut group2: Vec<(usize, &'a YtCandidate, Option<u64>)> = Vec::new();

    for (idx, cand) in candidates.iter().enumerate() {
        if let Some(actual_ms) = cand.duration_ms() {
            let diff = (expected_duration_ms as i64 - actual_ms as i64).unsigned_abs();
            if diff <= tolerance_ms {
                group1.push((idx, cand, diff));
            } else {
                group2.push((idx, cand, Some(diff)));
            }
        } else {
            group2.push((idx, cand, None));
        }
    }

    // Sort group 1: lowest duration difference first; break ties with original index
    group1.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));

    // Sort group 2: lowest duration difference first if known, otherwise original index
    group2.sort_by(|a, b| match (a.2, b.2) {
        (Some(d_a), Some(d_b)) => d_a.cmp(&d_b).then_with(|| a.0.cmp(&b.0)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    });

    let mut result = Vec::with_capacity(candidates.len());
    for (_, cand, _) in group1 {
        result.push(cand);
    }
    for (_, cand, _) in group2 {
        result.push(cand);
    }

    result
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
        let mut cmd = Command::new(&self.binary_path);
        cmd.env("PATH", get_augmented_path());
        cmd.args(detect_js_runtime_args());
        cmd.arg("--dump-json")
            .arg("--no-playlist")
            .arg("--default-search")
            .arg("auto")
            .arg(&search_spec);

        let output = cmd
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
    /// 2. Searches candidates and ranks them (duration tolerance ±15s, closest match first)
    /// 3. Downloads candidate audio into `cache_dir`, falling back to next candidates on failure
    /// 4. Returns the path of the downloaded file
    pub fn match_and_download(
        &self,
        track: &SpotifyTrack,
        cache_dir: &Path,
    ) -> Result<PathBuf, DownloadError> {
        std::fs::create_dir_all(cache_dir)?;

        let query = build_search_query_for_track(track);
        let candidates = self.search_candidates(&query, 10)?;

        if candidates.is_empty() {
            return Err(DownloadError::NoMatchingCandidate {
                title: track.title.clone(),
                expected_ms: track.duration_ms,
                candidates_checked: 0,
            });
        }

        let ranked = rank_candidates(&candidates, track.duration_ms, self.tolerance_ms);
        if ranked.is_empty() {
            return Err(DownloadError::NoMatchingCandidate {
                title: track.title.clone(),
                expected_ms: track.duration_ms,
                candidates_checked: candidates.len(),
            });
        }

        let mut errors = Vec::new();

        for candidate in &ranked {
            let candidate_title = candidate.title.as_deref().unwrap_or("Unknown");
            let url = candidate
                .webpage_url
                .clone()
                .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", candidate.id));

            // Output template using track ID
            let output_template = cache_dir.join(format!("{}.%(ext)s", track.id));

            let mut cmd = Command::new(&self.binary_path);
            cmd.env("PATH", get_augmented_path());
            cmd.args(detect_js_runtime_args());
            cmd.arg("-f")
                .arg("ba/b")
                .arg("--no-playlist")
                .arg("-o")
                .arg(output_template.to_string_lossy().as_ref())
                .arg("--print")
                .arg("after_move:filepath")
                .arg(&url);

            let download_output = match cmd.output() {
                Ok(out) => out,
                Err(e) => {
                    let err_msg = format!("Failed to spawn yt-dlp: {}", e);
                    eprintln!(
                        "Warning: Download candidate {} ({}) failed: {}. Trying next candidate...",
                        candidate.id, candidate_title, err_msg
                    );
                    errors.push(format!(
                        "Candidate {} ({}): {}",
                        candidate.id, candidate_title, err_msg
                    ));
                    continue;
                }
            };

            if download_output.status.success() {
                let printed_path = String::from_utf8_lossy(&download_output.stdout)
                    .trim()
                    .to_string();
                if !printed_path.is_empty() {
                    let p = PathBuf::from(printed_path);
                    if p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                        return Ok(p);
                    }
                }

                // Fallback check: find any file in cache_dir starting with track.id
                let mut found_path = None;
                if let Ok(entries) = std::fs::read_dir(cache_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(stem) = path.file_stem() {
                            if stem == track.id.as_str()
                                && path.is_file()
                                && path.metadata().map(|m| m.len() > 0).unwrap_or(false)
                            {
                                found_path = Some(path);
                                break;
                            }
                        }
                    }
                }

                if let Some(path) = found_path {
                    return Ok(path);
                }

                let err_msg =
                    "yt-dlp reported success but downloaded file is missing or 0 bytes".to_string();
                eprintln!(
                    "Warning: Download candidate {} ({}) failed: {}. Trying next candidate...",
                    candidate.id, candidate_title, err_msg
                );
                errors.push(format!(
                    "Candidate {} ({}): {}",
                    candidate.id, candidate_title, err_msg
                ));
            } else {
                let stderr = String::from_utf8_lossy(&download_output.stderr)
                    .trim()
                    .to_string();
                let err_msg = if stderr.is_empty() {
                    format!("yt-dlp exited with status {}", download_output.status)
                } else {
                    stderr
                };
                eprintln!(
                    "Warning: Download candidate {} ({}) failed: {}. Trying next candidate...",
                    candidate.id, candidate_title, err_msg
                );
                errors.push(format!(
                    "Candidate {} ({}): {}",
                    candidate.id, candidate_title, err_msg
                ));
            }

            // Clean up any 0-byte ghost files from failed attempt
            if let Ok(entries) = std::fs::read_dir(cache_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(stem) = path.file_stem() {
                        if stem == track.id.as_str()
                            && path.is_file()
                            && path.metadata().map(|m| m.len() == 0).unwrap_or(false)
                        {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            }
        }

        Err(DownloadError::ExecutionFailed(format!(
            "All {} download candidates failed for track '{}':\n{}",
            ranked.len(),
            track.title,
            errors.join("\n")
        )))
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

        // Within +15 seconds (15000 ms)
        assert!(check_duration_tolerance(spotify_ms, 215_000));
        // Within -15 seconds
        assert!(check_duration_tolerance(spotify_ms, 185_000));

        // 1 ms beyond boundary (+15001 ms) -> false
        assert!(!check_duration_tolerance(spotify_ms, 215_001));
        // 1 ms beyond boundary (-15001 ms) -> false
        assert!(!check_duration_tolerance(spotify_ms, 184_999));

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

    #[test]
    fn test_get_augmented_path() {
        let aug = get_augmented_path();
        assert!(!aug.is_empty(), "Augmented PATH should not be empty");

        let paths: Vec<PathBuf> = std::env::split_paths(&aug).collect();
        assert!(!paths.is_empty(), "Parsed PATH entries should not be empty");

        // Verify no duplicates
        let mut seen = std::collections::HashSet::new();
        for p in &paths {
            assert!(
                seen.insert(p.clone()),
                "Duplicate path entry found in get_augmented_path: {:?}",
                p
            );
        }

        // Verify standard paths on Unix/macOS
        #[cfg(unix)]
        {
            assert!(
                paths.contains(&PathBuf::from("/usr/bin")),
                "Must contain /usr/bin"
            );
            assert!(paths.contains(&PathBuf::from("/bin")), "Must contain /bin");
            assert!(
                paths.contains(&PathBuf::from("/usr/local/bin")),
                "Must contain /usr/local/bin"
            );
            assert!(
                paths.contains(&PathBuf::from("/opt/homebrew/bin")),
                "Must contain /opt/homebrew/bin"
            );
        }

        if let Some(home) = dirs::home_dir() {
            let spotyburn_bin = home.join(".spotyburn").join("bin");
            assert_eq!(
                paths.first(),
                Some(&spotyburn_bin),
                "~/.spotyburn/bin must be the first entry in augmented PATH"
            );
        }
    }

    #[test]
    fn test_rank_candidates() {
        let expected_duration_ms = 200_000; // 200 seconds
        let tolerance_ms = 15_000; // ±15 seconds

        let candidates = vec![
            YtCandidate {
                id: "c_diff_10".to_string(),
                title: Some("Candidate Diff 10s".to_string()),
                duration: Some(210.0), // 210s -> diff 10s (Group 1)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_exact".to_string(),
                title: Some("Candidate Exact".to_string()),
                duration: Some(200.0), // 200s -> diff 0s (Group 1)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_diff_2".to_string(),
                title: Some("Candidate Diff 2s".to_string()),
                duration: Some(198.0), // 198s -> diff 2s (Group 1)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_diff_15_boundary".to_string(),
                title: Some("Candidate Diff 15s Boundary".to_string()),
                duration: Some(215.0), // 215s -> diff 15s (Group 1)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_diff_16_out".to_string(),
                title: Some("Candidate Diff 16s Out".to_string()),
                duration: Some(216.0), // 216s -> diff 16s (Group 2)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_diff_60_out".to_string(),
                title: Some("Candidate Diff 60s Out".to_string()),
                duration: Some(260.0), // 260s -> diff 60s (Group 2)
                webpage_url: None,
            },
            YtCandidate {
                id: "c_no_duration_1".to_string(),
                title: Some("Candidate Unknown Duration 1".to_string()),
                duration: None, // Group 2, unknown
                webpage_url: None,
            },
            YtCandidate {
                id: "c_no_duration_2".to_string(),
                title: Some("Candidate Unknown Duration 2".to_string()),
                duration: None, // Group 2, unknown
                webpage_url: None,
            },
            YtCandidate {
                id: "c_diff_2_second".to_string(),
                title: Some("Candidate Diff 2s Second".to_string()),
                duration: Some(202.0), // 202s -> diff 2s (Group 1, tie with c_diff_2)
                webpage_url: None,
            },
        ];

        let ranked = rank_candidates(&candidates, expected_duration_ms, tolerance_ms);
        let ranked_ids: Vec<&str> = ranked.iter().map(|c| c.id.as_str()).collect();

        assert_eq!(
            ranked_ids,
            vec![
                "c_exact",            // diff 0s (Group 1)
                "c_diff_2",           // diff 2s (Group 1, first)
                "c_diff_2_second",    // diff 2s (Group 1, second by original index)
                "c_diff_10",          // diff 10s (Group 1)
                "c_diff_15_boundary", // diff 15s (Group 1, exact tolerance boundary)
                "c_diff_16_out",      // diff 16s (Group 2, lowest diff)
                "c_diff_60_out",      // diff 60s (Group 2)
                "c_no_duration_1",    // unknown duration (Group 2, first by original index)
                "c_no_duration_2",    // unknown duration (Group 2, second by original index)
            ]
        );
    }

    #[test]
    fn test_rank_candidates_empty() {
        let ranked = rank_candidates(&[], 200_000, 15_000);
        assert!(ranked.is_empty());
    }

    #[test]
    fn test_detect_js_runtime_args() {
        let args = detect_js_runtime_args();
        // If deno is installed, args should be empty.
        // If only node is installed, args should contain --js-runtimes node:...
        if !args.is_empty() {
            assert_eq!(args[0], "--js-runtimes");
            assert!(args[1].starts_with("node:"));
        }
    }
}
