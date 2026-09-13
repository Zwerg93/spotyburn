use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::downloader::get_augmented_path;
use crate::models::SpotifyTrack;

/// Red Book Audio CD-DA standard constants
pub const REDBOOK_SAMPLE_RATE: u32 = 44_100;
pub const REDBOOK_CHANNELS: u16 = 2;
pub const REDBOOK_BITS_PER_SAMPLE: u16 = 16;
pub const REDBOOK_BYTES_PER_SAMPLE_FRAME: usize = 4; // 2 channels * 2 bytes (16-bit)
pub const REDBOOK_SECTOR_SIZE: usize = 2352; // 1/75th of a second
pub const REDBOOK_SECTORS_PER_SECOND: u32 = 75;
pub const REDBOOK_SAMPLES_PER_SECTOR: usize = 588; // 588 * 4 bytes = 2352 bytes
pub const REDBOOK_BYTES_PER_SECOND: u32 = 176_400; // 44100 * 4 bytes

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("FFmpeg binary not found in PATH or ~/.spotyburn/bin")]
    FFmpegNotFound,

    #[error("FFmpeg execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Invalid WAV format: {0}")]
    InvalidWav(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Calculates the number of padding bytes required to align `data_len` to the
/// Red Book CD-DA sector boundary (2352 bytes).
pub fn calculate_sector_padding(data_len: u64) -> u64 {
    let remainder = data_len % (REDBOOK_SECTOR_SIZE as u64);
    if remainder == 0 {
        0
    } else {
        (REDBOOK_SECTOR_SIZE as u64) - remainder
    }
}

/// Calculates the total number of whole 2352-byte sectors required for `data_len`
/// including sector padding.
pub fn calculate_total_sectors(data_len: u64) -> u64 {
    let padding = calculate_sector_padding(data_len);
    (data_len + padding) / (REDBOOK_SECTOR_SIZE as u64)
}

/// Converts a count of 2352-byte Red Book sectors (1/75 s) to duration in milliseconds.
pub fn sectors_to_duration_ms(sectors: u64) -> u64 {
    // 75 sectors = 1000 ms -> ms = (sectors * 1000) / 75
    (sectors * 1000) / (REDBOOK_SECTORS_PER_SECOND as u64)
}

/// Converts duration in milliseconds to the minimum number of Red Book sectors (rounded up).
pub fn duration_ms_to_sectors(duration_ms: u64) -> u64 {
    (duration_ms * (REDBOOK_SECTORS_PER_SECOND as u64)).div_ceil(1000)
}

/// Locates the `ffmpeg` executable on the host system:
/// 1. Optional explicit hint / `SPOTYBURN_FFMPEG_PATH` environment variable.
/// 2. `~/.spotyburn/bin/ffmpeg` (or `.exe` on Windows).
/// 3. In the system `PATH` or standard Unix directories.
pub fn find_ffmpeg() -> Result<PathBuf, AudioError> {
    find_ffmpeg_with_hint(None)
}

/// Locates `ffmpeg` with an optional custom path hint.
pub fn find_ffmpeg_with_hint(hint: Option<&Path>) -> Result<PathBuf, AudioError> {
    if let Some(custom) = hint {
        if custom.is_file() {
            return Ok(custom.to_path_buf());
        }
    }

    if let Ok(env_path) = std::env::var("SPOTYBURN_FFMPEG_PATH") {
        let p = PathBuf::from(env_path);
        if p.is_file() {
            return Ok(p);
        }
    }

    let bin_name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    // Check ~/.spotyburn/bin/ffmpeg
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".spotyburn").join("bin").join(bin_name);
        if local_bin.is_file() {
            return Ok(local_bin);
        }
    }

    // Check common Unix paths
    if !cfg!(windows) {
        let common_paths = [
            Path::new("/opt/homebrew/bin/ffmpeg"),
            Path::new("/usr/local/bin/ffmpeg"),
            Path::new("/usr/bin/ffmpeg"),
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

    Err(AudioError::FFmpegNotFound)
}

/// Inspects a WAV in-memory buffer and appends zero padding to the `data` chunk so that
/// the audio PCM length is an exact multiple of 2352 bytes. Updates both the RIFF and data chunk sizes.
pub fn pad_wav_buffer(wav_bytes: &[u8]) -> Result<Vec<u8>, AudioError> {
    if wav_bytes.len() < 12 {
        return Err(AudioError::InvalidWav("WAV header too short".to_string()));
    }
    if &wav_bytes[0..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
        return Err(AudioError::InvalidWav(
            "Missing RIFF/WAVE header".to_string(),
        ));
    }

    let mut offset = 12;
    let mut data_chunk_info: Option<(usize, usize, u32)> = None; // (header_offset, content_offset, current_size)

    while offset + 8 <= wav_bytes.len() {
        let chunk_id = &wav_bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            wav_bytes[offset + 4],
            wav_bytes[offset + 5],
            wav_bytes[offset + 6],
            wav_bytes[offset + 7],
        ]);

        if chunk_id == b"data" {
            data_chunk_info = Some((offset, offset + 8, chunk_size));
            break;
        }

        // Advance to next chunk (aligned to even boundary per RIFF spec)
        let padded_size = chunk_size as usize + (chunk_size as usize % 2);
        offset += 8 + padded_size;
    }

    let (chunk_header_offset, data_content_offset, data_size) = data_chunk_info
        .ok_or_else(|| AudioError::InvalidWav("Missing 'data' chunk in WAV".to_string()))?;

    let padding = calculate_sector_padding(data_size as u64) as usize;
    if padding == 0 {
        return Ok(wav_bytes.to_vec());
    }

    let new_data_size = data_size
        .checked_add(padding as u32)
        .ok_or_else(|| AudioError::InvalidWav("WAV size overflow".to_string()))?;

    let current_riff_size =
        u32::from_le_bytes([wav_bytes[4], wav_bytes[5], wav_bytes[6], wav_bytes[7]]);
    let new_riff_size = current_riff_size
        .checked_add(padding as u32)
        .ok_or_else(|| AudioError::InvalidWav("RIFF size overflow".to_string()))?;

    let mut result = Vec::with_capacity(wav_bytes.len() + padding);

    // 1. Copy RIFF header with updated size
    result.extend_from_slice(&wav_bytes[0..4]);
    result.extend_from_slice(&new_riff_size.to_le_bytes());
    result.extend_from_slice(&wav_bytes[8..chunk_header_offset + 4]);

    // 2. Write updated data chunk size
    result.extend_from_slice(&new_data_size.to_le_bytes());

    // 3. Write data chunk content
    let data_end = data_content_offset + (data_size as usize);
    if data_end > wav_bytes.len() {
        return Err(AudioError::InvalidWav(
            "WAV data chunk truncated".to_string(),
        ));
    }
    result.extend_from_slice(&wav_bytes[data_content_offset..data_end]);

    // 4. Append padding zeros (Red Book silence)
    result.resize(result.len() + padding, 0u8);

    // 5. Append any trailing chunks (e.g. metadata or tags)
    if data_end < wav_bytes.len() {
        result.extend_from_slice(&wav_bytes[data_end..]);
    }

    Ok(result)
}

/// Aligns a WAV file on disk to the Red Book 2352-byte sector boundary.
/// Returns the number of padding bytes added.
pub fn pad_wav_file_to_sector_boundary(path: &Path) -> Result<u64, AudioError> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    let padded = pad_wav_buffer(&buffer)?;
    let added_bytes = (padded.len() - buffer.len()) as u64;

    if added_bytes > 0 {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&padded)?;
        file.set_len(padded.len() as u64)?;
    }

    Ok(added_bytes)
}

/// Transcoder orchestrator using FFmpeg
#[derive(Debug, Clone)]
pub struct FfmpegTranscoder {
    pub binary_path: PathBuf,
}

impl FfmpegTranscoder {
    pub fn new() -> Result<Self, AudioError> {
        let binary_path = find_ffmpeg()?;
        Ok(Self { binary_path })
    }

    pub fn with_binary(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }

    /// Converts an input audio file into Red Book CD-DA WAV:
    /// - 44,100 Hz sampling rate
    /// - 16-Bit signed PCM (`pcm_s16le`)
    /// - 2 channels (stereo)
    /// - Optional EBU R128 loudness normalization (`loudnorm=I=-16:TP=-1.0:LRA=11`)
    /// - Strict 2352-byte sector alignment padding applied post-conversion
    pub fn convert_to_redbook_wav(
        &self,
        input: &Path,
        output: &Path,
        normalize: bool,
    ) -> Result<(), AudioError> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.env("PATH", get_augmented_path());
        cmd.arg("-y"); // Overwrite output file
        cmd.arg("-i").arg(input);

        if normalize {
            // Standard EBU R128 loudness normalization for music CD-DA
            // Target: -16 LUFS, True Peak: -1.0 dBFS, LRA: 11 LU
            cmd.arg("-af").arg("loudnorm=I=-16:TP=-1.0:LRA=11");
        }

        cmd.arg("-ar").arg(REDBOOK_SAMPLE_RATE.to_string());
        cmd.arg("-ac").arg(REDBOOK_CHANNELS.to_string());
        cmd.arg("-c:a").arg("pcm_s16le");
        cmd.arg("-f").arg("wav");
        cmd.arg(output);

        let output_res = cmd
            .output()
            .map_err(|e| AudioError::ExecutionFailed(e.to_string()))?;

        if !output_res.status.success() {
            let stderr = String::from_utf8_lossy(&output_res.stderr);
            return Err(AudioError::ExecutionFailed(stderr.into_owned()));
        }

        // Apply Red Book sector alignment padding
        pad_wav_file_to_sector_boundary(output)?;

        Ok(())
    }

    /// Converts an input audio file into an MP3 file with ID3v2 metadata:
    /// - `libmp3lame` audio encoder
    /// - Specified bitrate in kbps (e.g. 320k)
    /// - 44,100 Hz sampling rate
    /// - ID3v2 metadata: title, artist, album, track number
    pub fn convert_to_mp3(
        &self,
        input: &Path,
        output: &Path,
        bitrate_kbps: u32,
        track: &SpotifyTrack,
    ) -> Result<(), AudioError> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.env("PATH", get_augmented_path());
        let args = build_mp3_command_args(input, output, bitrate_kbps, track);
        cmd.args(&args);

        let output_res = cmd
            .output()
            .map_err(|e| AudioError::ExecutionFailed(e.to_string()))?;

        if !output_res.status.success() {
            let stderr = String::from_utf8_lossy(&output_res.stderr);
            return Err(AudioError::ExecutionFailed(stderr.into_owned()));
        }

        Ok(())
    }
}

/// Generates the FFmpeg command line arguments for MP3 encoding with ID3 tags
pub fn build_mp3_command_args(
    input: &Path,
    output: &Path,
    bitrate_kbps: u32,
    track: &SpotifyTrack,
) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        input.to_string_lossy().to_string(),
        "-c:a".to_string(),
        "libmp3lame".to_string(),
        "-b:a".to_string(),
        format!("{}k", bitrate_kbps),
        "-ar".to_string(),
        "44100".to_string(),
        "-metadata".to_string(),
        format!("title={}", track.title),
        "-metadata".to_string(),
        format!("artist={}", track.artists.join(", ")),
        "-metadata".to_string(),
        format!("album={}", track.album),
        "-metadata".to_string(),
        format!("track={}", track.track_number),
    ];
    args.push(output.to_string_lossy().to_string());
    args
}

/// Helper function to create a minimal canonical PCM WAV buffer for testing.
pub fn create_test_wav_buffer(pcm_data_len: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(44 + pcm_data_len);
    let riff_size = (36 + pcm_data_len) as u32;

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt subchunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size
    buf.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = PCM
    buf.extend_from_slice(&REDBOOK_CHANNELS.to_le_bytes()); // 2 channels
    buf.extend_from_slice(&REDBOOK_SAMPLE_RATE.to_le_bytes()); // 44100
    let byte_rate = REDBOOK_BYTES_PER_SECOND;
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = REDBOOK_CHANNELS * (REDBOOK_BITS_PER_SAMPLE / 8);
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&REDBOOK_BITS_PER_SAMPLE.to_le_bytes());

    // data subchunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&(pcm_data_len as u32).to_le_bytes());
    buf.resize(44 + pcm_data_len, 0x11); // Fill with dummy audio bytes

    buf
}

/// Standalone convenience function matching spec 3.2
pub fn convert_to_redbook_wav(
    input: &Path,
    output: &Path,
    normalize: bool,
) -> Result<(), AudioError> {
    let transcoder = FfmpegTranscoder::new()?;
    transcoder.convert_to_redbook_wav(input, output, normalize)
}

/// Standalone convenience function for MP3 conversion
pub fn convert_to_mp3(
    input: &Path,
    output: &Path,
    bitrate_kbps: u32,
    track: &SpotifyTrack,
) -> Result<(), AudioError> {
    let transcoder = FfmpegTranscoder::new()?;
    transcoder.convert_to_mp3(input, output, bitrate_kbps, track)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_sector_padding() {
        // Exactly 0 remainder -> 0 padding
        assert_eq!(calculate_sector_padding(0), 0);
        assert_eq!(calculate_sector_padding(2352), 0);
        assert_eq!(calculate_sector_padding(2352 * 10), 0);

        // 1 byte into sector -> 2351 bytes padding
        assert_eq!(calculate_sector_padding(1), 2351);
        assert_eq!(calculate_sector_padding(2353), 2351);

        // 2351 bytes into sector -> 1 byte padding
        assert_eq!(calculate_sector_padding(2351), 1);
        assert_eq!(calculate_sector_padding(2352 * 2 - 1), 1);

        // Half sector (1176 bytes) -> 1176 bytes padding
        assert_eq!(calculate_sector_padding(1176), 1176);
    }

    #[test]
    fn test_calculate_total_sectors() {
        assert_eq!(calculate_total_sectors(0), 0);
        assert_eq!(calculate_total_sectors(1), 1);
        assert_eq!(calculate_total_sectors(2352), 1);
        assert_eq!(calculate_total_sectors(2353), 2);
        assert_eq!(calculate_total_sectors(2352 * 75), 75); // Exactly 1 second
    }

    #[test]
    fn test_sector_duration_conversions() {
        // 75 sectors = 1 second = 1000 ms
        assert_eq!(sectors_to_duration_ms(75), 1000);
        // 150 sectors = 2 seconds = 2000 ms (standard pregap)
        assert_eq!(sectors_to_duration_ms(150), 2000);

        // 1000 ms -> 75 sectors
        assert_eq!(duration_ms_to_sectors(1000), 75);
        // 2000 ms -> 150 sectors
        assert_eq!(duration_ms_to_sectors(2000), 150);
    }

    #[test]
    fn test_pad_wav_buffer_already_aligned() {
        let wav_data = create_test_wav_buffer(2352 * 2);
        let padded = pad_wav_buffer(&wav_data).expect("Padding should succeed");
        assert_eq!(padded.len(), wav_data.len());
        assert_eq!(padded, wav_data);
    }

    #[test]
    fn test_pad_wav_buffer_unaligned() {
        // Data length: 1000 bytes (needs 1352 bytes padding to reach 2352)
        let wav_data = create_test_wav_buffer(1000);
        let padded = pad_wav_buffer(&wav_data).expect("Padding should succeed");

        let expected_padding = 2352 - 1000;
        assert_eq!(padded.len(), wav_data.len() + expected_padding);

        // Check new RIFF size
        let riff_size = u32::from_le_bytes([padded[4], padded[5], padded[6], padded[7]]);
        assert_eq!(riff_size as usize, padded.len() - 8);

        // Check new data chunk size
        let data_size = u32::from_le_bytes([padded[40], padded[41], padded[42], padded[43]]);
        assert_eq!(data_size, 2352);
        assert_eq!(data_size % (REDBOOK_SECTOR_SIZE as u32), 0);

        // Check that padded bytes are zero
        assert!(padded[44 + 1000..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_pad_wav_buffer_with_trailing_chunk() {
        let mut wav_data = create_test_wav_buffer(1000);
        // Append a dummy trailing chunk (e.g. LIST)
        let trailing_chunk = b"LIST\x04\x00\x00\x00INFO";
        wav_data.extend_from_slice(trailing_chunk);
        // Update original RIFF size
        let initial_riff = (wav_data.len() - 8) as u32;
        wav_data[4..8].copy_from_slice(&initial_riff.to_le_bytes());

        let padded = pad_wav_buffer(&wav_data).expect("Padding should succeed");
        let expected_padding = 2352 - 1000;
        assert_eq!(padded.len(), wav_data.len() + expected_padding);

        // Data chunk should now be 2352 bytes
        let data_size = u32::from_le_bytes([padded[40], padded[41], padded[42], padded[43]]);
        assert_eq!(data_size, 2352);

        // Trailing chunk should still be at the end
        assert_eq!(&padded[padded.len() - 12..], trailing_chunk);
    }

    #[test]
    fn test_pad_wav_buffer_invalid() {
        let invalid = b"NOT_A_WAV_FILE";
        assert!(pad_wav_buffer(invalid).is_err());
    }

    #[test]
    fn test_pad_wav_file_on_disk() {
        let temp_dir = std::env::temp_dir();
        let test_wav = temp_dir.join("test_pad_sector.wav");
        let initial_wav = create_test_wav_buffer(3000); // 3000 % 2352 = 648 -> padding = 1704
        std::fs::write(&test_wav, &initial_wav).expect("Writing test WAV");

        let added = pad_wav_file_to_sector_boundary(&test_wav).expect("Padding file");
        assert_eq!(added, 1704);

        let final_bytes = std::fs::read(&test_wav).expect("Reading padded file");
        assert_eq!(final_bytes.len(), initial_wav.len() + 1704);

        // Second run should add 0 bytes
        let added_second = pad_wav_file_to_sector_boundary(&test_wav).expect("Second padding run");
        assert_eq!(added_second, 0);

        let _ = std::fs::remove_file(test_wav);
    }

    #[test]
    fn test_find_ffmpeg_with_hint() {
        let temp_dir = std::env::temp_dir();
        let dummy_bin = temp_dir.join("dummy_ffmpeg_test_bin");
        std::fs::write(&dummy_bin, b"test").expect("Failed to write dummy binary");

        let found = find_ffmpeg_with_hint(Some(&dummy_bin));
        assert!(found.is_ok());
        assert_eq!(found.unwrap(), dummy_bin);

        let _ = std::fs::remove_file(dummy_bin);
    }

    #[test]
    fn test_build_mp3_command_args() {
        let track = SpotifyTrack {
            id: "track123".to_string(),
            title: "Bohemian Rhapsody".to_string(),
            artists: vec!["Queen".to_string(), "Freddie Mercury".to_string()],
            album: "A Night at the Opera".to_string(),
            duration_ms: 354000,
            track_number: 11,
            isrc: Some("GBUM71029604".to_string()),
        };

        let input = Path::new("/tmp/input.webm");
        let output = Path::new("/tmp/output.mp3");
        let args = build_mp3_command_args(input, output, 320, &track);

        assert_eq!(args[0], "-y");
        assert_eq!(args[1], "-i");
        assert_eq!(args[2], "/tmp/input.webm");
        assert_eq!(args[3], "-c:a");
        assert_eq!(args[4], "libmp3lame");
        assert_eq!(args[5], "-b:a");
        assert_eq!(args[6], "320k");
        assert_eq!(args[7], "-ar");
        assert_eq!(args[8], "44100");

        assert!(args.contains(&"title=Bohemian Rhapsody".to_string()));
        assert!(args.contains(&"artist=Queen, Freddie Mercury".to_string()));
        assert!(args.contains(&"album=A Night at the Opera".to_string()));
        assert!(args.contains(&"track=11".to_string()));
        assert_eq!(args.last().unwrap(), "/tmp/output.mp3");
    }
}
