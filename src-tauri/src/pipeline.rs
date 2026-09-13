use std::path::{Path, PathBuf};

use crate::audio::{self, AudioError, FfmpegTranscoder};
use crate::cuesheet::{self, CueError, TrackAudio};
use crate::downloader::{self, DownloadError, YtDlpDownloader};
use crate::models::SpotifyTrack;

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("Download error: {0}")]
    Download(#[from] DownloadError),

    #[error("Audio transcoding error: {0}")]
    Audio(#[from] AudioError),

    #[error("CUE sheet error: {0}")]
    Cue(#[from] CueError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Unified Audio Pipeline orchestration matching spec 3.2
pub struct AudioPipeline {
    pub downloader: Option<YtDlpDownloader>,
    pub transcoder: Option<FfmpegTranscoder>,
}

impl AudioPipeline {
    /// Attempts to create a pipeline using auto-discovered system or local binaries
    pub fn new() -> Self {
        Self {
            downloader: YtDlpDownloader::new().ok(),
            transcoder: FfmpegTranscoder::new().ok(),
        }
    }

    /// Creates an `AudioPipeline` with explicit binary paths
    pub fn with_binaries(yt_dlp_path: Option<PathBuf>, ffmpeg_path: Option<PathBuf>) -> Self {
        Self {
            downloader: yt_dlp_path.map(YtDlpDownloader::with_binary),
            transcoder: ffmpeg_path.map(FfmpegTranscoder::with_binary),
        }
    }

    /// Slices through yt-dlp to find and download a SpotifyTrack matching duration tolerance (±5s)
    pub fn match_and_download(
        &self,
        track: &SpotifyTrack,
        cache_dir: &Path,
    ) -> Result<PathBuf, DownloadError> {
        if let Some(dl) = &self.downloader {
            dl.match_and_download(track, cache_dir)
        } else {
            downloader::match_and_download(track, cache_dir)
        }
    }

    /// Converts an input audio file into Red Book CD-DA WAV (44.1kHz, 16-bit stereo PCM, 2352-byte sector aligned)
    pub fn convert_to_redbook_wav(
        &self,
        input: &Path,
        output: &Path,
        normalize: bool,
    ) -> Result<(), AudioError> {
        if let Some(tr) = &self.transcoder {
            tr.convert_to_redbook_wav(input, output, normalize)
        } else {
            audio::convert_to_redbook_wav(input, output, normalize)
        }
    }

    /// Converts an input audio file into an MP3 file with ID3 tags
    pub fn convert_to_mp3(
        &self,
        input: &Path,
        output: &Path,
        bitrate_kbps: u32,
        track: &SpotifyTrack,
    ) -> Result<(), AudioError> {
        if let Some(tr) = &self.transcoder {
            tr.convert_to_mp3(input, output, bitrate_kbps, track)
        } else {
            audio::convert_to_mp3(input, output, bitrate_kbps, track)
        }
    }

    /// Generates a standard-compliant Red Book CUE-Sheet with CD-Text
    pub fn generate_cuesheet(
        &self,
        tracks: &[TrackAudio],
        output_cue: &Path,
    ) -> Result<(), CueError> {
        cuesheet::generate_cuesheet(tracks, output_cue)
    }

    /// Complete end-to-end processing of a track:
    /// 1. Match & download audio track via yt-dlp
    /// 2. Convert and sector-align into Red Book CD-DA WAV via ffmpeg
    pub fn process_track(
        &self,
        track: &SpotifyTrack,
        cache_dir: &Path,
        output_wav: &Path,
        normalize: bool,
    ) -> Result<PathBuf, PipelineError> {
        let raw_download = self.match_and_download(track, cache_dir)?;
        self.convert_to_redbook_wav(&raw_download, output_wav, normalize)?;
        Ok(output_wav.to_path_buf())
    }
}

impl Default for AudioPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_instantiation() {
        let pipeline = AudioPipeline::new();
        // Even if external binaries are not installed on test host, creation succeeds gracefully
        let _ = pipeline.downloader.as_ref();
        let _ = pipeline.transcoder.as_ref();
    }

    #[test]
    fn test_pipeline_with_custom_binaries() {
        let pipeline = AudioPipeline::with_binaries(
            Some(PathBuf::from("/custom/yt-dlp")),
            Some(PathBuf::from("/custom/ffmpeg")),
        );
        assert!(pipeline.downloader.is_some());
        assert!(pipeline.transcoder.is_some());
    }
}
