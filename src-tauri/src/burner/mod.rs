use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

use crate::spotify::SpotifyTrack;

pub mod linux;
pub mod macos;
pub mod windows;

/// Standard Red Book Audio CD capacity limit: 80 minutes (4,800,000 milliseconds)
pub const RED_BOOK_MAX_DURATION_MS: u64 = 4_800_000;
pub const RED_BOOK_MAX_MINUTES: f64 = 80.0;
pub const RED_BOOK_MAX_BLOCKS: u64 = 360_000; // 80 min * 60 sec * 75 frames/sec
pub const BYTES_PER_SECTOR: u64 = 2352;
pub const SECTORS_PER_SECOND: u64 = 75;

/// Standard Data CD capacity limit: 700 MB (734,003,200 Bytes)
pub const DATA_CD_MAX_BYTES: u64 = 700 * 1024 * 1024;
pub const DATA_CD_MAX_MB: f64 = 700.0;

/// Estimated MP3 256 kbit/s byte rate: 32 bytes per millisecond (~1.92 MB/min)
pub const MP3_256K_BYTES_PER_MS: u64 = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpticalDrive {
    pub id: String, // e.g. "1", "/dev/sr0", "0"
    pub vendor: String,
    pub product: String,
    pub interconnect: String, // USB, SATA, ATAPI
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BurnMode {
    #[default]
    AudioCdRedBook, // max 80 Minuten, 44.1kHz 16-bit PCM WAV
    DataMp3Cd,  // max 700 MB, MP3 Dateien
    ExportOnly, // Virtueller Test/Export ohne physikalischen Brenner
}

impl BurnMode {
    pub fn label(&self) -> &'static str {
        match self {
            BurnMode::AudioCdRedBook => "Audio CD (Red Book)",
            BurnMode::DataMp3Cd => "Data MP3 CD",
            BurnMode::ExportOnly => "Export Only",
        }
    }
}

impl std::fmt::Display for BurnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaStatus {
    pub drive_id: String,
    pub media_present: bool,
    pub is_blank: bool,
    pub media_type: String, // "CD-R", "CD-RW", "None"
    pub free_blocks: u64,
    pub free_minutes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BurnOptions {
    pub speed: u32,
    pub eject_after: bool,
    pub simulate: bool,
    pub burn_mode: BurnMode,
}

impl Default for BurnOptions {
    fn default() -> Self {
        Self {
            speed: 0,
            eject_after: true,
            simulate: false,
            burn_mode: BurnMode::AudioCdRedBook,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BurnProgress {
    pub stage: String, // "Preparing", "Writing", "Closing", "Finished"
    pub percent: f32,  // 0.0 .. 100.0
    pub current_track: Option<u32>,
    pub total_tracks: Option<u32>,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Clone)]
pub enum BurnError {
    #[error("Optical drive '{0}' not found")]
    DriveNotFound(String),

    #[error("No media present in drive '{0}'")]
    NoMedia(String),

    #[error("Media in drive '{0}' is not blank or writable")]
    MediaNotWritable(String),

    #[error("Audio CD capacity exceeded: requested {requested_ms} ms ({requested_minutes:.2} min), but Red Book limit is {max_ms} ms ({max_minutes:.2} min)")]
    CapacityExceeded {
        requested_ms: u64,
        max_ms: u64,
        requested_minutes: f64,
        max_minutes: f64,
    },

    #[error("Data CD capacity exceeded: requested {requested_bytes} bytes ({requested_mb:.2} MB), but limit is {max_bytes} bytes ({max_mb:.2} MB)")]
    DataCapacityExceeded {
        requested_bytes: u64,
        max_bytes: u64,
        requested_mb: f64,
        max_mb: f64,
    },

    #[error("Drive operation failed: {0}")]
    ExecutionFailed(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Platform unsupported: {0}")]
    Unsupported(String),
}

impl From<std::io::Error> for BurnError {
    fn from(err: std::io::Error) -> Self {
        BurnError::IoError(err.to_string())
    }
}

/// Validates that total duration does not exceed Red Book Audio CD limit (80 minutes / 4,800,000 ms).
pub fn validate_audio_capacity(duration_ms: u64) -> Result<(), BurnError> {
    if duration_ms > RED_BOOK_MAX_DURATION_MS {
        Err(BurnError::CapacityExceeded {
            requested_ms: duration_ms,
            max_ms: RED_BOOK_MAX_DURATION_MS,
            requested_minutes: duration_ms as f64 / 60_000.0,
            max_minutes: RED_BOOK_MAX_MINUTES,
        })
    } else {
        Ok(())
    }
}

/// Validates total duration of playlist tracks in milliseconds.
pub fn validate_playlist_capacity(track_durations_ms: &[u64]) -> Result<u64, BurnError> {
    let total_ms: u64 = track_durations_ms.iter().sum();
    validate_audio_capacity(total_ms)?;
    Ok(total_ms)
}

/// Validates data disc size against 700 MB limit.
pub fn validate_data_capacity(total_bytes: u64) -> Result<(), BurnError> {
    if total_bytes > DATA_CD_MAX_BYTES {
        Err(BurnError::DataCapacityExceeded {
            requested_bytes: total_bytes,
            max_bytes: DATA_CD_MAX_BYTES,
            requested_mb: total_bytes as f64 / (1024.0 * 1024.0),
            max_mb: DATA_CD_MAX_MB,
        })
    } else {
        Ok(())
    }
}

/// Calculates remaining Audio CD capacity in milliseconds.
pub fn audio_capacity_remaining_ms(used_ms: u64) -> i64 {
    RED_BOOK_MAX_DURATION_MS as i64 - used_ms as i64
}

/// Calculates capacity percentage used (0.0 to 100.0+).
pub fn audio_capacity_percent(used_ms: u64) -> f64 {
    (used_ms as f64 / RED_BOOK_MAX_DURATION_MS as f64) * 100.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapacityUsage {
    pub used_ms: u64,
    pub max_ms: u64,
    pub used_bytes: u64,
    pub max_bytes: u64,
    pub used_percent: f64,
    pub is_exceeded: bool,
    pub mode_label: String,
}

pub fn calculate_capacity_usage(tracks: &[SpotifyTrack], mode: BurnMode) -> CapacityUsage {
    let used_ms: u64 = tracks.iter().map(|t| t.duration_ms).sum();

    match mode {
        BurnMode::AudioCdRedBook => {
            let max_ms = RED_BOOK_MAX_DURATION_MS;
            let used_bytes = (used_ms * 176_400) / 1000;
            let max_bytes = (RED_BOOK_MAX_DURATION_MS * 176_400) / 1000;
            let used_percent = if max_ms > 0 {
                (used_ms as f64 / max_ms as f64) * 100.0
            } else {
                0.0
            };
            let is_exceeded = used_ms > max_ms;
            CapacityUsage {
                used_ms,
                max_ms,
                used_bytes,
                max_bytes,
                used_percent,
                is_exceeded,
                mode_label: mode.label().to_string(),
            }
        }
        BurnMode::DataMp3Cd => {
            let used_bytes = used_ms * MP3_256K_BYTES_PER_MS;
            let max_bytes = DATA_CD_MAX_BYTES;
            let max_ms = DATA_CD_MAX_BYTES / MP3_256K_BYTES_PER_MS;
            let used_percent = if max_bytes > 0 {
                (used_bytes as f64 / max_bytes as f64) * 100.0
            } else {
                0.0
            };
            let is_exceeded = used_bytes > max_bytes;
            CapacityUsage {
                used_ms,
                max_ms,
                used_bytes,
                max_bytes,
                used_percent,
                is_exceeded,
                mode_label: mode.label().to_string(),
            }
        }
        BurnMode::ExportOnly => {
            let used_bytes = (used_ms * 176_400) / 1000;
            CapacityUsage {
                used_ms,
                max_ms: 0,
                used_bytes,
                max_bytes: 0,
                used_percent: 0.0,
                is_exceeded: false,
                mode_label: mode.label().to_string(),
            }
        }
    }
}

pub trait DiscBurner: Send + Sync {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError>;
    fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError>;
    fn burn_audio_cd(
        &self,
        drive_id: &str,
        tracks_or_cue: &Path,
        speed: u32,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError>;
    fn burn_data_cd(
        &self,
        drive_id: &str,
        files_dir: &Path,
        speed: u32,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError>;
    fn eject(&self, drive_id: &str) -> Result<(), BurnError>;

    fn burn_audio_cd_with_options(
        &self,
        drive_id: &str,
        tracks_or_cue: &Path,
        options: &BurnOptions,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        self.burn_audio_cd(drive_id, tracks_or_cue, options.speed, progress_cb)?;
        if options.eject_after {
            let _ = self.eject(drive_id);
        }
        Ok(())
    }

    fn burn_data_cd_with_options(
        &self,
        drive_id: &str,
        files_dir: &Path,
        options: &BurnOptions,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        self.burn_data_cd(drive_id, files_dir, options.speed, progress_cb)?;
        if options.eject_after {
            let _ = self.eject(drive_id);
        }
        Ok(())
    }
}

/// Stub/fallback burner when running on an unsupported OS or for testing.
#[derive(Debug, Default, Clone)]
pub struct StubBurner {
    pub reason: String,
}

impl StubBurner {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl DiscBurner for StubBurner {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError> {
        Err(BurnError::Unsupported(self.reason.clone()))
    }

    fn get_media_status(&self, _drive_id: &str) -> Result<MediaStatus, BurnError> {
        Err(BurnError::Unsupported(self.reason.clone()))
    }

    fn burn_audio_cd(
        &self,
        _drive_id: &str,
        _tracks_or_cue: &Path,
        _speed: u32,
        _progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        Err(BurnError::Unsupported(self.reason.clone()))
    }

    fn burn_data_cd(
        &self,
        _drive_id: &str,
        _files_dir: &Path,
        _speed: u32,
        _progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        Err(BurnError::Unsupported(self.reason.clone()))
    }

    fn eject(&self, _drive_id: &str) -> Result<(), BurnError> {
        Err(BurnError::Unsupported(self.reason.clone()))
    }
}

/// Factory function instantiating the native burner implementation for the host OS.
pub fn create_burner() -> Box<dyn DiscBurner> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosBurner::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsBurner::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxBurner::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Box::new(StubBurner::new("Platform not supported"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capacity_validation_red_book() {
        // Exactly 80 minutes
        assert!(validate_audio_capacity(4_800_000).is_ok());

        // Less than 80 minutes
        assert!(validate_audio_capacity(4_799_999).is_ok());
        assert!(validate_audio_capacity(0).is_ok());
        assert!(validate_audio_capacity(3_600_000).is_ok()); // 60 minutes

        // More than 80 minutes
        let err = validate_audio_capacity(4_800_001).unwrap_err();
        match err {
            BurnError::CapacityExceeded {
                requested_ms,
                max_ms,
                ..
            } => {
                assert_eq!(requested_ms, 4_800_001);
                assert_eq!(max_ms, 4_800_000);
            }
            _ => panic!("Expected CapacityExceeded error"),
        }

        // 81 minutes
        let err81 = validate_audio_capacity(4_860_000).unwrap_err();
        assert!(matches!(err81, BurnError::CapacityExceeded { .. }));
    }

    #[test]
    fn test_playlist_capacity_validation() {
        let tracks = vec![180_000, 200_000, 240_000]; // 620,000 ms (~10.3 min)
        let total = validate_playlist_capacity(&tracks).unwrap();
        assert_eq!(total, 620_000);

        let long_tracks = vec![2_400_000, 2_400_001]; // 4,800,001 ms
        assert!(validate_playlist_capacity(&long_tracks).is_err());
    }

    #[test]
    fn test_audio_capacity_remaining_and_percent() {
        let remaining = audio_capacity_remaining_ms(3_600_000);
        assert_eq!(remaining, 1_200_000);

        let pct = audio_capacity_percent(2_400_000);
        assert!((pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_data_capacity_validation() {
        assert!(validate_data_capacity(700 * 1024 * 1024).is_ok());
        assert!(validate_data_capacity(700 * 1024 * 1024 + 1).is_err());
    }

    #[test]
    fn test_stub_burner() {
        let burner = StubBurner::new("testing stub");
        assert!(burner.detect_drives().is_err());
        assert!(burner.get_media_status("0").is_err());
        assert!(burner.eject("0").is_err());
    }

    #[test]
    fn test_create_burner() {
        let burner = create_burner();
        // Should instantiate without panicking
        let _ = burner;
    }

    struct MockBurner {
        ejected: std::sync::atomic::AtomicBool,
    }

    impl DiscBurner for MockBurner {
        fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError> {
            Ok(vec![OpticalDrive {
                id: "mock-1".into(),
                vendor: "MockVendor".into(),
                product: "MockProduct".into(),
                interconnect: "USB".into(),
            }])
        }

        fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError> {
            Ok(MediaStatus {
                drive_id: drive_id.into(),
                media_present: true,
                is_blank: true,
                media_type: "CD-R".into(),
                free_blocks: 360000,
                free_minutes: 80.0,
            })
        }

        fn burn_audio_cd(
            &self,
            _drive_id: &str,
            _tracks_or_cue: &Path,
            _speed: u32,
            progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
        ) -> Result<(), BurnError> {
            progress_cb(BurnProgress {
                stage: "Finished".into(),
                percent: 100.0,
                current_track: Some(1),
                total_tracks: Some(1),
                message: "Mock done".into(),
            });
            Ok(())
        }

        fn burn_data_cd(
            &self,
            _drive_id: &str,
            _files_dir: &Path,
            _speed: u32,
            progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
        ) -> Result<(), BurnError> {
            progress_cb(BurnProgress {
                stage: "Finished".into(),
                percent: 100.0,
                current_track: None,
                total_tracks: None,
                message: "Mock data done".into(),
            });
            Ok(())
        }

        fn eject(&self, _drive_id: &str) -> Result<(), BurnError> {
            self.ejected
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn test_mock_burner_with_options() {
        let burner = MockBurner {
            ejected: std::sync::atomic::AtomicBool::new(false),
        };

        let drives = burner.detect_drives().unwrap();
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].id, "mock-1");

        let status = burner.get_media_status("mock-1").unwrap();
        assert!(status.is_blank);

        let options = BurnOptions {
            speed: 16,
            eject_after: true,
            simulate: false,
            burn_mode: BurnMode::AudioCdRedBook,
        };

        let progress_called = std::sync::atomic::AtomicBool::new(false);
        let res =
            burner.burn_audio_cd_with_options("mock-1", Path::new("/dummy/cue"), &options, &|_p| {
                progress_called.store(true, std::sync::atomic::Ordering::SeqCst);
            });

        assert!(res.is_ok());
        assert!(progress_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(burner.ejected.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_burn_mode_labels_and_display() {
        assert_eq!(BurnMode::AudioCdRedBook.label(), "Audio CD (Red Book)");
        assert_eq!(BurnMode::DataMp3Cd.label(), "Data MP3 CD");
        assert_eq!(BurnMode::ExportOnly.label(), "Export Only");
        assert_eq!(format!("{}", BurnMode::ExportOnly), "Export Only");
    }

    #[test]
    fn test_calculate_capacity_usage_audio_cd() {
        let tracks = vec![
            SpotifyTrack {
                id: "t1".into(),
                title: "Track 1".into(),
                artists: vec!["Artist".into()],
                album: "Album".into(),
                duration_ms: 2_400_000, // 40 min
                track_number: 1,
                isrc: None,
            },
            SpotifyTrack {
                id: "t2".into(),
                title: "Track 2".into(),
                artists: vec!["Artist".into()],
                album: "Album".into(),
                duration_ms: 1_200_000, // 20 min
                track_number: 2,
                isrc: None,
            },
        ];

        let usage = calculate_capacity_usage(&tracks, BurnMode::AudioCdRedBook);
        assert_eq!(usage.used_ms, 3_600_000);
        assert_eq!(usage.max_ms, 4_800_000);
        assert_eq!(usage.used_percent, 75.0);
        assert!(!usage.is_exceeded);
        assert_eq!(usage.mode_label, "Audio CD (Red Book)");

        // Exceeded Audio CD (> 80 min = 4_800_000 ms)
        let overlong = vec![SpotifyTrack {
            id: "t3".into(),
            title: "Long Track".into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 5_000_000,
            track_number: 1,
            isrc: None,
        }];
        let usage_over = calculate_capacity_usage(&overlong, BurnMode::AudioCdRedBook);
        assert!(usage_over.is_exceeded);
        assert!(usage_over.used_percent > 100.0);
    }

    #[test]
    fn test_calculate_capacity_usage_data_mp3_cd() {
        // 10,000,000 ms * 32 bytes/ms = 320,000,000 bytes (< 734,003,200 bytes limit)
        let tracks = vec![SpotifyTrack {
            id: "t1".into(),
            title: "MP3 Track".into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 10_000_000,
            track_number: 1,
            isrc: None,
        }];
        let usage = calculate_capacity_usage(&tracks, BurnMode::DataMp3Cd);
        assert_eq!(usage.used_ms, 10_000_000);
        assert_eq!(usage.used_bytes, 320_000_000);
        assert_eq!(usage.max_bytes, DATA_CD_MAX_BYTES);
        assert_eq!(usage.max_ms, DATA_CD_MAX_BYTES / MP3_256K_BYTES_PER_MS);
        assert!(!usage.is_exceeded);
        assert_eq!(usage.mode_label, "Data MP3 CD");

        // Exceeding 700 MB: 25_000_000 ms * 32 = 800,000,000 bytes > 734,003,200
        let overlong = vec![SpotifyTrack {
            id: "t2".into(),
            title: "Huge MP3 Set".into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 25_000_000,
            track_number: 1,
            isrc: None,
        }];
        let usage_over = calculate_capacity_usage(&overlong, BurnMode::DataMp3Cd);
        assert!(usage_over.is_exceeded);
        assert!(usage_over.used_percent > 100.0);
    }

    #[test]
    fn test_calculate_capacity_usage_export_only() {
        // Even with 50_000_000 ms, export only should never be exceeded
        let tracks = vec![SpotifyTrack {
            id: "t1".into(),
            title: "Huge Concert".into(),
            artists: vec!["Artist".into()],
            album: "Album".into(),
            duration_ms: 50_000_000,
            track_number: 1,
            isrc: None,
        }];
        let usage = calculate_capacity_usage(&tracks, BurnMode::ExportOnly);
        assert_eq!(usage.used_ms, 50_000_000);
        assert_eq!(usage.max_ms, 0);
        assert_eq!(usage.max_bytes, 0);
        assert_eq!(usage.used_percent, 0.0);
        assert!(!usage.is_exceeded);
        assert_eq!(usage.mode_label, "Export Only");
    }
}
