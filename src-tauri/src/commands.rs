use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Emitter;

use crate::burner::{
    self, calculate_capacity_usage, BurnMode, BurnOptions, CapacityUsage, MediaStatus, OpticalDrive,
};
use crate::config::AppConfig;
use crate::cuesheet::{self, TrackAudio};
use crate::models::{SpotifyPlaylistSummary, SpotifySearchResult, SpotifyTrack, UserProfile};
use crate::pipeline::AudioPipeline;
use crate::spotify::SpotifyClient;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpotifyFetchResult {
    pub tracks: Vec<SpotifyTrack>,
    pub total_duration_ms: u64,
    pub total_duration_formatted: String,
    pub track_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BurnProgressPayload {
    pub stage: String,
    pub percent: f32,
    pub current_track: Option<u32>,
    pub total_tracks: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BurnLogPayload {
    pub level: String, // "info", "warn", "error", "success"
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BurnFinishedPayload {
    pub success: bool,
    pub message: String,
    pub total_tracks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BurnErrorPayload {
    pub stage: String,
    pub error: String,
}

pub fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

pub fn current_timestamp() -> String {
    let now = std::time::SystemTime::now();
    let since_epoch = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_of_day = since_epoch % 86400;
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;
    let seconds = secs_of_day % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}

pub fn validate_burn_request(
    tracks: &[SpotifyTrack],
    burn_mode: BurnMode,
    drive_id: Option<&str>,
) -> Result<u64, String> {
    if tracks.is_empty() {
        return Err("No tracks selected for burning.".to_string());
    }

    if burn_mode != BurnMode::ExportOnly {
        match drive_id {
            Some(id) if !id.trim().is_empty() => {}
            _ => return Err("Optical drive required for burning mode.".to_string()),
        }
    }

    let total_ms: u64 = tracks.iter().map(|t| t.duration_ms).sum();

    match burn_mode {
        BurnMode::AudioCdRedBook => {
            burner::validate_audio_capacity(total_ms).map_err(|e| e.to_string())?;
        }
        BurnMode::DataMp3Cd => {
            let total_bytes = total_ms * burner::MP3_256K_BYTES_PER_MS;
            burner::validate_data_capacity(total_bytes).map_err(|e| e.to_string())?;
        }
        BurnMode::ExportOnly => {
            // Virtual export has no media capacity constraints
        }
    }

    Ok(total_ms)
}

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to SpotyBurn.", name)
}

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    AppConfig::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    // Preserve auth tokens from the existing config if the incoming config doesn't include them.
    // The frontend Settings modal only sends client_id, client_secret, cache_dir, default_burn_mode
    // and would otherwise overwrite the persisted refresh_token/user_access_token/user_display_name
    // with None, forcing the user to re-authenticate on every app restart.
    let mut merged = config;
    if merged.refresh_token.is_none()
        || merged.user_access_token.is_none()
        || merged.user_display_name.is_none()
    {
        if let Ok(existing) = AppConfig::load() {
            if merged.refresh_token.is_none() {
                merged.refresh_token = existing.refresh_token;
            }
            if merged.user_access_token.is_none() {
                merged.user_access_token = existing.user_access_token;
            }
            if merged.user_display_name.is_none() {
                merged.user_display_name = existing.user_display_name;
            }
        }
    }
    merged.save().map_err(|e| e.to_string())
}

fn get_spotify_credentials(config: &AppConfig) -> (String, String) {
    let client_id = if !config.client_id.trim().is_empty() {
        config.client_id.trim().to_string()
    } else {
        std::env::var("SPOTIFY_CLIENT_ID").unwrap_or_default()
    };

    let client_secret = if !config.client_secret.trim().is_empty() {
        config.client_secret.trim().to_string()
    } else {
        std::env::var("SPOTIFY_CLIENT_SECRET").unwrap_or_default()
    };

    (client_id, client_secret)
}

#[tauri::command]
pub async fn spotify_login() -> Result<UserProfile, String> {
    let mut config = AppConfig::load().unwrap_or_default();
    let (client_id, client_secret) = get_spotify_credentials(&config);

    let tokens = crate::spotify::start_oauth_loopback(
        &client_id,
        &client_secret,
        crate::spotify::SPOTIFY_OAUTH_PORT,
    )
    .await
    .map_err(|e| e.to_string())?;

    let client =
        SpotifyClient::with_refresh_token(client_id, client_secret, tokens.refresh_token.clone());
    let profile = client
        .fetch_current_user_profile()
        .await
        .map_err(|e| e.to_string())?;

    config.refresh_token = Some(tokens.refresh_token);
    config.user_access_token = Some(tokens.access_token);
    config.user_display_name = Some(profile.display_name.clone());
    let _ = config.save();

    Ok(profile)
}

#[tauri::command]
pub async fn spotify_logout() -> Result<(), String> {
    let mut config = AppConfig::load().unwrap_or_default();
    config.refresh_token = None;
    config.user_access_token = None;
    config.user_display_name = None;
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_user_profile() -> Result<Option<UserProfile>, String> {
    let config = AppConfig::load().unwrap_or_default();
    let refresh_token = match &config.refresh_token {
        Some(rt) if !rt.trim().is_empty() => rt.clone(),
        _ => return Ok(None),
    };

    let (client_id, client_secret) = get_spotify_credentials(&config);

    let client = SpotifyClient::with_refresh_token(client_id, client_secret, refresh_token);
    match client.fetch_current_user_profile().await {
        Ok(profile) => Ok(Some(profile)),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn get_user_playlists() -> Result<Vec<SpotifyPlaylistSummary>, String> {
    let config = AppConfig::load().unwrap_or_default();
    let refresh_token = match &config.refresh_token {
        Some(rt) if !rt.trim().is_empty() => rt.clone(),
        _ => return Err("Not logged in to Spotify".to_string()),
    };

    let (client_id, client_secret) = get_spotify_credentials(&config);

    let client = SpotifyClient::with_refresh_token(client_id, client_secret, refresh_token);
    client
        .fetch_user_playlists()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_spotify_tracks(url: String) -> Result<SpotifyFetchResult, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Spotify URL or URI cannot be empty".to_string());
    }

    let config = AppConfig::load().unwrap_or_default();
    let (client_id, client_secret) = get_spotify_credentials(&config);

    let client = SpotifyClient::with_tokens(
        client_id,
        client_secret,
        config.refresh_token.clone(),
        config.user_access_token.clone(),
    );
    let tracks = client.fetch(trimmed).await.map_err(|e| e.to_string())?;

    let total_duration_ms: u64 = tracks.iter().map(|t| t.duration_ms).sum();
    let track_count = tracks.len();
    let total_duration_formatted = format_duration_ms(total_duration_ms);

    Ok(SpotifyFetchResult {
        tracks,
        total_duration_ms,
        total_duration_formatted,
        track_count,
    })
}

#[tauri::command]
pub async fn search_spotify(
    query: String,
    search_type: Option<String>,
    limit: Option<u32>,
) -> Result<SpotifySearchResult, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(SpotifySearchResult::default());
    }

    let config = AppConfig::load().unwrap_or_default();
    let (client_id, client_secret) = get_spotify_credentials(&config);

    let client = SpotifyClient::with_tokens(
        client_id,
        client_secret,
        config.refresh_token,
        config.user_access_token,
    );

    let types: Vec<&str> = match search_type.as_deref() {
        Some("track") | Some("tracks") => vec!["track"],
        Some("playlist") | Some("playlists") => vec!["playlist"],
        Some("album") | Some("albums") => vec!["album"],
        Some(custom) if !custom.trim().is_empty() && custom != "all" => {
            vec![custom.trim()]
        }
        _ => vec!["track", "playlist", "album"],
    };

    let search_limit = limit.unwrap_or(10).clamp(1, 10);
    client
        .search(trimmed, &types, search_limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_optical_drives() -> Result<Vec<OpticalDrive>, String> {
    let burner = burner::create_burner();
    burner.detect_drives().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_media_status(drive_id: String) -> Result<MediaStatus, String> {
    let burner = burner::create_burner();
    burner
        .get_media_status(&drive_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn eject_drive(drive_id: String) -> Result<(), String> {
    let burner = burner::create_burner();
    burner.eject(&drive_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn calculate_capacity(tracks: Vec<SpotifyTrack>, mode: BurnMode) -> CapacityUsage {
    calculate_capacity_usage(&tracks, mode)
}

pub fn open_in_file_manager(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open directory in Finder: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open directory in Explorer: {e}"))?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open directory in file manager: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_cache_folder() -> Result<String, String> {
    let config = AppConfig::load().unwrap_or_default();
    let cache_dir = config.cache_dir;
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            format!(
                "Failed to create cache directory '{}': {e}",
                cache_dir.display()
            )
        })?;
    }
    open_in_file_manager(&cache_dir)?;
    Ok(cache_dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn select_destination_folder() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg("POSIX path of (choose folder with prompt \"SpotyBurn: Zielordner auswählen\")")
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.FolderBrowserDialog; if ($f.ShowDialog() -eq 'OK') { $f.SelectedPath }"])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let output = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--directory",
                "--title=SpotyBurn: Zielordner auswählen",
            ])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[tauri::command]
pub async fn start_burn_job(
    app: tauri::AppHandle,
    tracks: Vec<SpotifyTrack>,
    drive_id: Option<String>,
    burn_mode: Option<BurnMode>,
    speed: Option<u32>,
    eject_after: Option<bool>,
    simulate: Option<bool>,
) -> Result<String, String> {
    let mode = burn_mode.unwrap_or_default();
    let burn_speed = speed.unwrap_or(0);
    let eject = eject_after.unwrap_or(true);
    let is_sim = simulate.unwrap_or(false);

    // Initial capacity, empty & drive validation
    validate_burn_request(&tracks, mode, drive_id.as_deref())?;

    let app_handle = app.clone();
    let target_drive = drive_id.unwrap_or_default();
    let job_tracks = tracks.clone();

    tokio::spawn(async move {
        run_burn_pipeline(
            app_handle,
            job_tracks,
            target_drive,
            mode,
            burn_speed,
            eject,
            is_sim,
        )
        .await;
    });

    Ok("Burn job started successfully".to_string())
}

async fn run_burn_pipeline(
    app: tauri::AppHandle,
    tracks: Vec<SpotifyTrack>,
    drive_id: String,
    burn_mode: BurnMode,
    speed: u32,
    eject_after: bool,
    simulate: bool,
) {
    let total_tracks = tracks.len();

    let emit_log = |level: &str, msg: &str| {
        let _ = app.emit(
            "burn-log",
            BurnLogPayload {
                level: level.to_string(),
                message: msg.to_string(),
                timestamp: current_timestamp(),
            },
        );
    };

    let emit_progress = |stage: &str, pct: f32, cur: Option<u32>, tot: Option<u32>, msg: &str| {
        let _ = app.emit(
            "burn-progress",
            BurnProgressPayload {
                stage: stage.to_string(),
                percent: pct,
                current_track: cur,
                total_tracks: tot,
                message: msg.to_string(),
            },
        );
    };

    if burn_mode == BurnMode::ExportOnly {
        emit_log(
            "info",
            &format!(
                "Initializing export pipeline for {} tracks (Mode: ExportOnly)...",
                total_tracks
            ),
        );
    } else {
        emit_log(
            "info",
            &format!(
                "Initializing pipeline for {} tracks on drive '{}' (Mode: {:?}, Speed: {}x)...",
                total_tracks, drive_id, burn_mode, speed
            ),
        );
    }
    emit_progress(
        "Preparing",
        2.0,
        None,
        Some(total_tracks as u32),
        "Setting up workspace...",
    );

    let config = AppConfig::load().unwrap_or_default();
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let job_dir: PathBuf = config.cache_dir.join(format!("job_{}", timestamp_ms));

    if let Err(e) = std::fs::create_dir_all(&job_dir) {
        let err_msg = format!("Failed to create workspace directory: {e}");
        emit_log("error", &err_msg);
        let _ = app.emit(
            "burn-error",
            BurnErrorPayload {
                stage: "Workspace".into(),
                error: err_msg,
            },
        );
        return;
    }

    let pipeline = std::sync::Arc::new(AudioPipeline::new());
    let mut audio_tracks = Vec::with_capacity(total_tracks);

    // 1. Download & Transcode Tracks
    for (idx, track) in tracks.iter().enumerate() {
        let track_num = (idx + 1) as u32;
        let artist_display = if track.artists.is_empty() {
            "Unknown Artist".to_string()
        } else {
            track.artists.join(", ")
        };
        let desc = format!("{} - {}", artist_display, track.title);

        let dl_pct = 5.0 + ((idx as f32) / (total_tracks as f32)) * 25.0;
        emit_log(
            "info",
            &format!(
                "[{}/{}] Sourcing audio for: {}",
                track_num, total_tracks, desc
            ),
        );
        emit_progress(
            "Downloading",
            dl_pct,
            Some(track_num),
            Some(total_tracks as u32),
            &format!("Downloading [{}/{}]: {}", track_num, total_tracks, desc),
        );

        let clean_artist = sanitize_filename(&artist_display);
        let clean_title = sanitize_filename(&track.title);
        let wav_filename = if clean_artist.is_empty() || clean_artist == "Unknown Artist" {
            format!("{:02} - {}.wav", track_num, clean_title)
        } else {
            format!("{:02} - {} - {}.wav", track_num, clean_artist, clean_title)
        };
        let wav_path = job_dir.join(&wav_filename);

        // Real sourcing via yt-dlp (run on blocking thread pool to not starve Tokio runtime)
        let dl_pipeline = pipeline.clone();
        let dl_track = track.clone();
        let dl_dir = job_dir.clone();
        let raw_file = match tokio::task::spawn_blocking(move || {
            dl_pipeline.match_and_download(&dl_track, &dl_dir)
        })
        .await
        {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                emit_log(
                    "warn",
                    &format!(
                        "[{}/{}] ⚠ Übersprungen – Download fehlgeschlagen für '{}': {e}",
                        track_num, total_tracks, track.title
                    ),
                );
                continue; // skip this track, continue with next
            }
            Err(e) => {
                emit_log(
                    "warn",
                    &format!(
                        "[{}/{}] ⚠ Übersprungen – Download-Task abgestürzt für '{}': {e}",
                        track_num, total_tracks, track.title
                    ),
                );
                continue;
            }
        };

        let tc_pct = 30.0 + ((idx as f32) / (total_tracks as f32)) * 25.0;
        emit_log(
            "info",
            &format!(
                "[{}/{}] Transcoding to Red Book 44.1kHz 16-bit PCM WAV...",
                track_num, total_tracks
            ),
        );
        emit_progress(
            "Transcoding",
            tc_pct,
            Some(track_num),
            Some(total_tracks as u32),
            &format!("Transcoding [{}/{}]: {}", track_num, total_tracks, desc),
        );

        // Transcode on blocking thread pool
        let tc_pipeline = pipeline.clone();
        let tc_raw = raw_file.clone();
        let tc_wav = wav_path.clone();
        match tokio::task::spawn_blocking(move || {
            tc_pipeline.convert_to_redbook_wav(&tc_raw, &tc_wav, true)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                emit_log(
                    "warn",
                    &format!(
                        "[{}/{}] ⚠ Übersprungen – Transcode fehlgeschlagen für '{}': {e}",
                        track_num, total_tracks, track.title
                    ),
                );
                continue;
            }
            Err(e) => {
                emit_log(
                    "warn",
                    &format!(
                        "[{}/{}] ⚠ Übersprungen – Transcode-Task abgestürzt für '{}': {e}",
                        track_num, total_tracks, track.title
                    ),
                );
                continue;
            }
        }

        let mut track_item = TrackAudio::from_spotify_track(track, &wav_filename);
        track_item.track_number = track_num;
        audio_tracks.push(track_item);
    }

    // 2. Generate CUE Sheet
    emit_log(
        "info",
        "Generating standard-compliant Red Book CUE sheet with CD-Text...",
    );
    emit_progress(
        "CUE Generation",
        58.0,
        None,
        Some(total_tracks as u32),
        "Generating CUE sheet...",
    );

    let cue_path = job_dir.join("disc.cue");
    if let Err(e) = cuesheet::generate_cuesheet(&audio_tracks, &cue_path) {
        let err_msg = format!("CUE sheet generation failed: {e}");
        emit_log("error", &err_msg);
        let _ = app.emit(
            "burn-error",
            BurnErrorPayload {
                stage: "CUE Generation".into(),
                error: err_msg,
            },
        );
        return;
    }
    emit_log(
        "info",
        &format!("CUE sheet written to: {}", cue_path.display()),
    );

    if burn_mode == BurnMode::ExportOnly {
        emit_log(
            "success",
            &format!(
                "Export completed successfully! Files saved to: {}",
                job_dir.display()
            ),
        );
        emit_progress(
            "Finished",
            100.0,
            None,
            Some(total_tracks as u32),
            "Export completed successfully!",
        );
        let _ = open_in_file_manager(&job_dir);
        let _ = app.emit(
            "burn-finished",
            BurnFinishedPayload {
                success: true,
                message: format!("Export completed successfully to {}", job_dir.display()),
                total_tracks,
            },
        );
        return;
    }

    // 3. Burn Process
    emit_log(
        "info",
        &format!(
            "Starting burner engine on drive '{}' (eject: {})...",
            drive_id, eject_after
        ),
    );
    emit_progress(
        "Writing",
        60.0,
        None,
        Some(total_tracks as u32),
        "Initiating burning process...",
    );

    if simulate {
        for step in [65, 75, 85, 95, 100] {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            emit_progress(
                "Writing",
                step as f32,
                None,
                Some(total_tracks as u32),
                &format!("Simulated burning progress: {}%", step),
            );
            emit_log("info", &format!("Simulated burn progress: {}%", step));
        }
        emit_log("success", "Simulation finished successfully.");
        let _ = app.emit(
            "burn-finished",
            BurnFinishedPayload {
                success: true,
                message: "Simulation completed successfully".into(),
                total_tracks,
            },
        );
        return;
    }

    let burner = burner::create_burner();
    let options = BurnOptions {
        speed,
        eject_after,
        simulate: false,
        burn_mode,
    };

    let app_clone = app.clone();
    let burn_res = match burn_mode {
        BurnMode::AudioCdRedBook => {
            burner.burn_audio_cd_with_options(&drive_id, &cue_path, &options, &move |prog| {
                let scaled_pct = 60.0 + (prog.percent * 0.40);
                let _ = app_clone.emit(
                    "burn-progress",
                    BurnProgressPayload {
                        stage: prog.stage.clone(),
                        percent: scaled_pct.min(100.0),
                        current_track: prog.current_track,
                        total_tracks: prog.total_tracks,
                        message: prog.message.clone(),
                    },
                );
                let _ = app_clone.emit(
                    "burn-log",
                    BurnLogPayload {
                        level: "info".into(),
                        message: prog.message,
                        timestamp: current_timestamp(),
                    },
                );
            })
        }
        BurnMode::DataMp3Cd => {
            burner.burn_data_cd_with_options(&drive_id, &job_dir, &options, &move |prog| {
                let scaled_pct = 60.0 + (prog.percent * 0.40);
                let _ = app_clone.emit(
                    "burn-progress",
                    BurnProgressPayload {
                        stage: prog.stage.clone(),
                        percent: scaled_pct.min(100.0),
                        current_track: prog.current_track,
                        total_tracks: prog.total_tracks,
                        message: prog.message.clone(),
                    },
                );
                let _ = app_clone.emit(
                    "burn-log",
                    BurnLogPayload {
                        level: "info".into(),
                        message: prog.message,
                        timestamp: current_timestamp(),
                    },
                );
            })
        }
        BurnMode::ExportOnly => Ok(()),
    };

    match burn_res {
        Ok(()) => {
            emit_log("success", "Disc burning completed successfully!");
            emit_progress(
                "Finished",
                100.0,
                None,
                Some(total_tracks as u32),
                "Burn completed successfully!",
            );
            let _ = app.emit(
                "burn-finished",
                BurnFinishedPayload {
                    success: true,
                    message: "Disc successfully burned!".into(),
                    total_tracks,
                },
            );
        }
        Err(e) => {
            let err_msg = format!("Burning failed: {e}");
            emit_log("error", &err_msg);
            let _ = app.emit(
                "burn-error",
                BurnErrorPayload {
                    stage: "Burning".into(),
                    error: err_msg,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration_ms(0), "00:00");
        assert_eq!(format_duration_ms(65_000), "01:05");
        assert_eq!(format_duration_ms(3_600_000), "01:00:00");
        assert_eq!(format_duration_ms(4_800_000), "01:20:00");
    }

    #[test]
    fn test_current_timestamp_format() {
        let ts = current_timestamp();
        assert_eq!(ts.len(), 8);
        assert_eq!(&ts[2..3], ":");
        assert_eq!(&ts[5..6], ":");
    }

    #[test]
    fn test_validate_burn_request_empty() {
        let res = validate_burn_request(&[], BurnMode::AudioCdRedBook, Some("0"));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("No tracks selected"));
    }

    #[test]
    fn test_validate_burn_request_capacity() {
        let normal_tracks = vec![SpotifyTrack {
            id: "1".into(),
            title: "Song 1".into(),
            artists: vec!["Artist 1".into()],
            album: "Album".into(),
            duration_ms: 3_000_000,
            track_number: 1,
            isrc: None,
        }];
        assert!(validate_burn_request(&normal_tracks, BurnMode::AudioCdRedBook, Some("0")).is_ok());

        // Without drive_id in Red Book mode, validation fails
        let res_no_drive = validate_burn_request(&normal_tracks, BurnMode::AudioCdRedBook, None);
        assert!(res_no_drive.is_err());
        assert!(res_no_drive.unwrap_err().contains("Optical drive required"));

        // With empty string drive_id in Red Book mode, validation fails
        let res_empty_drive =
            validate_burn_request(&normal_tracks, BurnMode::AudioCdRedBook, Some("  "));
        assert!(res_empty_drive.is_err());

        let overlong_tracks = vec![
            SpotifyTrack {
                id: "1".into(),
                title: "Song 1".into(),
                artists: vec!["Artist 1".into()],
                album: "Album".into(),
                duration_ms: 2_500_000,
                track_number: 1,
                isrc: None,
            },
            SpotifyTrack {
                id: "2".into(),
                title: "Song 2".into(),
                artists: vec!["Artist 2".into()],
                album: "Album".into(),
                duration_ms: 2_500_000,
                track_number: 2,
                isrc: None,
            },
        ];
        // 5,000,000 ms > 4,800,000 ms (80 mins)
        let res = validate_burn_request(&overlong_tracks, BurnMode::AudioCdRedBook, Some("0"));
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Audio CD capacity exceeded"));

        // But in DataMp3Cd mode, 5,000,000 ms audio is fine!
        assert!(validate_burn_request(&overlong_tracks, BurnMode::DataMp3Cd, Some("0")).is_ok());

        // In ExportOnly mode: drive_id is optional and capacity is unconstrained!
        assert!(validate_burn_request(&overlong_tracks, BurnMode::ExportOnly, None).is_ok());
        assert!(
            validate_burn_request(&overlong_tracks, BurnMode::ExportOnly, Some("optional")).is_ok()
        );
    }

    #[test]
    fn test_calculate_capacity_command() {
        let tracks = vec![SpotifyTrack {
            id: "1".into(),
            title: "Track 1".into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 60_000,
            track_number: 1,
            isrc: None,
        }];
        let usage_redbook = calculate_capacity(tracks.clone(), BurnMode::AudioCdRedBook);
        assert_eq!(usage_redbook.used_ms, 60_000);
        assert_eq!(usage_redbook.max_ms, 4_800_000);
        assert!(!usage_redbook.is_exceeded);

        let usage_export = calculate_capacity(tracks, BurnMode::ExportOnly);
        assert_eq!(usage_export.used_ms, 60_000);
        assert_eq!(usage_export.max_ms, 0);
        assert_eq!(usage_export.used_percent, 0.0);
        assert!(!usage_export.is_exceeded);
    }

    #[test]
    fn test_spotify_fetch_result_serialization() {
        let result = SpotifyFetchResult {
            tracks: vec![SpotifyTrack {
                id: "xyz123".into(),
                title: "Comfortably Numb".into(),
                artists: vec!["Pink Floyd".into()],
                album: "The Wall".into(),
                duration_ms: 382_000,
                track_number: 6,
                isrc: Some("GBAYE7900106".into()),
            }],
            total_duration_ms: 382_000,
            total_duration_formatted: "06:22".into(),
            track_count: 1,
        };

        let json = serde_json::to_string(&result).expect("Serialize SpotifyFetchResult");
        let parsed: SpotifyFetchResult =
            serde_json::from_str(&json).expect("Deserialize SpotifyFetchResult");
        assert_eq!(result, parsed);
    }

    #[test]
    fn test_burn_payloads_serialization() {
        let progress = BurnProgressPayload {
            stage: "Writing".into(),
            percent: 78.5,
            current_track: Some(3),
            total_tracks: Some(10),
            message: "Burning track 3 of 10...".into(),
        };
        let p_json = serde_json::to_string(&progress).expect("Serialize BurnProgressPayload");
        let p_deserialized: BurnProgressPayload = serde_json::from_str(&p_json).unwrap();
        assert_eq!(progress, p_deserialized);

        let log = BurnLogPayload {
            level: "info".into(),
            message: "Pipeline running".into(),
            timestamp: "12:34:56".into(),
        };
        let l_json = serde_json::to_string(&log).expect("Serialize BurnLogPayload");
        let l_deserialized: BurnLogPayload = serde_json::from_str(&l_json).unwrap();
        assert_eq!(log, l_deserialized);

        let finished = BurnFinishedPayload {
            success: true,
            message: "Burn complete".into(),
            total_tracks: 12,
        };
        let f_json = serde_json::to_string(&finished).expect("Serialize BurnFinishedPayload");
        let f_deserialized: BurnFinishedPayload = serde_json::from_str(&f_json).unwrap();
        assert_eq!(finished, f_deserialized);

        let error = BurnErrorPayload {
            stage: "Transcoding".into(),
            error: "FFmpeg exited with error".into(),
        };
        let e_json = serde_json::to_string(&error).expect("Serialize BurnErrorPayload");
        let e_deserialized: BurnErrorPayload = serde_json::from_str(&e_json).unwrap();
        assert_eq!(error, e_deserialized);
    }

    #[tokio::test]
    async fn test_fetch_spotify_tracks_empty_url() {
        let err = fetch_spotify_tracks("   ".to_string()).await.unwrap_err();
        assert!(err.contains("cannot be empty"));
    }

    static CONFIG_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn test_get_and_save_config() {
        let _lock = CONFIG_TEST_MUTEX.lock().unwrap();
        let temp_dir = std::env::temp_dir().join("spotyburn_cmd_test");
        let cfg = AppConfig {
            cache_dir: temp_dir.clone(),
            client_id: "test_client_id".into(),
            client_secret: "test_client_secret".into(),
            default_burn_mode: BurnMode::DataMp3Cd,
            ..AppConfig::default()
        };

        let save_res = save_config(cfg.clone()).await;
        assert!(save_res.is_ok());

        let loaded = get_config().await;
        assert!(loaded.is_ok());
        let loaded_cfg = loaded.unwrap();
        assert_eq!(loaded_cfg.client_id, "test_client_id");
        assert_eq!(loaded_cfg.client_secret, "test_client_secret");
        assert_eq!(loaded_cfg.default_burn_mode, BurnMode::DataMp3Cd);

        // Reset back to default
        let _ = save_config(AppConfig::default()).await;
    }

    #[tokio::test]
    async fn test_search_spotify_empty_query() {
        let res = search_spotify("   ".to_string(), None, None).await.unwrap();
        assert_eq!(res, SpotifySearchResult::default());
    }

    #[tokio::test]
    async fn test_spotify_logout_and_profile_status() {
        let _lock = CONFIG_TEST_MUTEX.lock().unwrap();
        // Prepare config with dummy tokens
        let mut cfg = AppConfig::default();
        cfg.refresh_token = Some("test_refresh_token".to_string());
        cfg.user_display_name = Some("TestUser".to_string());
        let _ = save_config(cfg).await;

        // Perform logout
        let logout_res = spotify_logout().await;
        assert!(logout_res.is_ok());

        // Check profile returns None
        let profile = get_user_profile().await.unwrap();
        assert_eq!(profile, None);

        // Check get_user_playlists returns error
        let playlists_res = get_user_playlists().await;
        assert!(playlists_res.is_err());
        assert!(playlists_res.unwrap_err().contains("Not logged in"));

        // Cleanup
        let _ = save_config(AppConfig::default()).await;
    }
}
