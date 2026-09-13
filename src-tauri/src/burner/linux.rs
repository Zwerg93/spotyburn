use std::fs;
#[cfg(target_os = "linux")]
use std::io::{BufRead, BufReader};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use super::{BurnError, BurnProgress, DiscBurner, MediaStatus, OpticalDrive};

#[derive(Debug, Default, Clone)]
pub struct LinuxBurner;

impl LinuxBurner {
    pub fn new() -> Self {
        Self
    }
}

/// Parses the `/proc/sys/dev/cdrom/info` file to discover connected optical drives.
pub fn parse_proc_cdrom_info(content: &str) -> Vec<OpticalDrive> {
    let mut drives = Vec::new();
    let mut drive_names = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("drive name:") {
            let parts: Vec<&str> = trimmed
                .trim_start_matches("drive name:")
                .split_whitespace()
                .collect();
            // In /proc/sys/dev/cdrom/info, drives are often listed in reverse order (e.g. sr1 sr0)
            for name in parts.into_iter().rev() {
                drive_names.push(name.to_string());
            }
            break;
        }
    }

    for name in drive_names {
        let dev_path = if name.starts_with('/') {
            name.clone()
        } else {
            format!("/dev/{name}")
        };

        // Attempt to read sysfs for vendor/model if available
        let sys_path = format!("/sys/class/block/{name}/device/model");
        let product = fs::read_to_string(&sys_path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Optical Drive".into());

        let sys_vendor = format!("/sys/class/block/{name}/device/vendor");
        let vendor = fs::read_to_string(&sys_vendor)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Generic".into());

        drives.push(OpticalDrive {
            id: dev_path,
            vendor,
            product,
            interconnect: "SATA".into(),
        });
    }

    drives
}

/// Parses the output of `wodim --devices` or `cdrecord --devices`.
/// Format:
/// ` 0  dev='/dev/sr0'    rwrw-- : 'HL-DT-ST' 'BD-RE WH16NS40'`
pub fn parse_wodim_devices(output: &str) -> Vec<OpticalDrive> {
    let mut drives = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("dev=") {
            continue;
        }

        let dev_id = if let Some(start) = trimmed.find("dev='") {
            let rest = &trimmed[start + 5..];
            if let Some(end) = rest.find('\'') {
                rest[..end].to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };

        // Extract strings enclosed in quotes after the colon ':'
        let mut vendor = "Generic".to_string();
        let mut product = "Optical Drive".to_string();

        if let Some(colon_pos) = trimmed.find(':') {
            let after_colon = &trimmed[colon_pos + 1..];
            let quoted_tokens: Vec<&str> = after_colon
                .split('\'')
                .enumerate()
                .filter(|(idx, _)| idx % 2 == 1)
                .map(|(_, s)| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            if let Some(&v) = quoted_tokens.first() {
                vendor = v.to_string();
            }
            if let Some(&p) = quoted_tokens.get(1) {
                product = p.to_string();
            }
        }

        drives.push(OpticalDrive {
            id: dev_id,
            vendor,
            product,
            interconnect: "SATA".into(),
        });
    }

    drives
}

/// Parses output of `wodim -prcap dev=...` or `wodim -toc dev=...`.
pub fn parse_wodim_media_status(output: &str, drive_id: &str) -> MediaStatus {
    let lower = output.to_lowercase();

    if lower.contains("no disk")
        || lower.contains("wrong disk")
        || lower.contains("cannot load media")
        || lower.contains("medium not present")
        || lower.contains("unit not ready")
    {
        return MediaStatus {
            drive_id: drive_id.to_string(),
            media_present: false,
            is_blank: false,
            media_type: "None".into(),
            free_blocks: 0,
            free_minutes: 0.0,
        };
    }

    let is_blank = lower.contains("blank") || lower.contains("disk status:    blank");
    let mut media_type = "CD-R".to_string();
    if lower.contains("cd-rw") {
        media_type = "CD-RW".to_string();
    } else if lower.contains("dvd-r") {
        media_type = "DVD-R".to_string();
    } else if lower.contains("dvd-rw") {
        media_type = "DVD-RW".to_string();
    }

    let mut free_blocks = if is_blank { 360_000 } else { 0 };

    for line in output.lines() {
        let l_lower = line.to_lowercase();
        if l_lower.contains("free blocks:") || l_lower.contains("available:") {
            if let Some(colon) = line.find(':') {
                let rest = &line[colon + 1..].trim();
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(blocks) = num_str.parse::<u64>() {
                    free_blocks = blocks;
                }
            }
        }
    }

    let free_minutes = (free_blocks as f64 / 4500.0 * 100.0).round() / 100.0;

    MediaStatus {
        drive_id: drive_id.to_string(),
        media_present: true,
        is_blank,
        media_type,
        free_blocks,
        free_minutes,
    }
}

/// Parses progress from `wodim` console output.
pub fn parse_wodim_progress(line: &str) -> Option<BurnProgress> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();

    if lower.contains("starting to write") || lower.contains("pre-formatting") {
        Some(BurnProgress {
            stage: "Preparing".into(),
            percent: 5.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.contains("fixating") || lower.contains("closing") {
        Some(BurnProgress {
            stage: "Closing".into(),
            percent: 95.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.contains("writing done") || lower.contains("burn completed") {
        Some(BurnProgress {
            stage: "Finished".into(),
            percent: 100.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.starts_with("track") {
        // e.g. "Track 01:   12 of   45 MB written (fifo 100%) [buf  98%]   4.5x."
        let mut curr_track = None;
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if let Some(t) = tokens.get(1) {
            let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(num) = digits.parse::<u32>() {
                curr_track = Some(num);
            }
        }

        // Percentage from "X of Y MB"
        let mut percent = 50.0f32;
        if let Some(of_idx) = tokens.iter().position(|&tok| tok == "of") {
            if of_idx > 0 && of_idx + 1 < tokens.len() {
                let curr_mb = tokens[of_idx - 1].parse::<f32>().ok();
                let tot_mb = tokens[of_idx + 1].parse::<f32>().ok();
                if let (Some(cur), Some(tot)) = (curr_mb, tot_mb) {
                    if tot > 0.0 {
                        percent = (cur / tot) * 90.0 + 5.0;
                    }
                }
            }
        }

        Some(BurnProgress {
            stage: "Writing".into(),
            percent,
            current_track: curr_track,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else {
        None
    }
}

impl DiscBurner for LinuxBurner {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError> {
        #[cfg(target_os = "linux")]
        {
            // First check /proc/sys/dev/cdrom/info
            if Path::new("/proc/sys/dev/cdrom/info").exists() {
                if let Ok(content) = fs::read_to_string("/proc/sys/dev/cdrom/info") {
                    let drives = parse_proc_cdrom_info(&content);
                    if !drives.is_empty() {
                        return Ok(drives);
                    }
                }
            }

            // Fallback to wodim --devices
            let output = Command::new("wodim").arg("--devices").output();
            if let Ok(out) = output {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let drives = parse_wodim_devices(&stdout);
                if !drives.is_empty() {
                    return Ok(drives);
                }
            }

            // Fallback to cdrecord --devices
            let output_cdr = Command::new("cdrecord").arg("--devices").output();
            if let Ok(out) = output_cdr {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let drives = parse_wodim_devices(&stdout);
                return Ok(drives);
            }

            Ok(Vec::new())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(BurnError::Unsupported(
                "Linux disc burner is only supported on Linux".into(),
            ))
        }
    }

    fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError> {
        #[cfg(target_os = "linux")]
        {
            let dev_arg = format!("dev={drive_id}");
            let output = Command::new("wodim")
                .args(["-prcap", &dev_arg])
                .output()
                .or_else(|_| Command::new("cdrecord").args(["-prcap", &dev_arg]).output())
                .map_err(|e| {
                    BurnError::ExecutionFailed(format!("Failed to query media status: {e}"))
                })?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}\n{stderr}");

            Ok(parse_wodim_media_status(&combined, drive_id))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "Linux disc burner is only supported on Linux".into(),
            ))
        }
    }

    fn burn_audio_cd(
        &self,
        drive_id: &str,
        tracks_or_cue: &Path,
        speed: u32,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        #[cfg(target_os = "linux")]
        {
            if !tracks_or_cue.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    tracks_or_cue.display()
                )));
            }

            let mut cmd = Command::new("wodim");
            cmd.args(["-v", "-dao", "-pad", "-audio"]);
            cmd.arg(format!("dev={drive_id}"));

            if speed > 0 {
                cmd.arg(format!("speed={speed}"));
            }

            // If directory, collect all .wav files sorted
            if tracks_or_cue.is_dir() {
                let mut wavs = Vec::new();
                for entry in fs::read_dir(tracks_or_cue)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "wav") {
                        wavs.push(path);
                    }
                }
                wavs.sort();
                for wav in wavs {
                    cmd.arg(wav);
                }
            } else {
                cmd.arg(tracks_or_cue);
            }

            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn wodim audio burn: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_wodim_progress(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for wodim failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "wodim burn failed with exit code: {status}"
                )));
            }

            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (drive_id, tracks_or_cue, speed, progress_cb);
            Err(BurnError::Unsupported(
                "Linux disc burner is only supported on Linux".into(),
            ))
        }
    }

    fn burn_data_cd(
        &self,
        drive_id: &str,
        files_dir: &Path,
        speed: u32,
        progress_cb: &(dyn Fn(BurnProgress) + Send + Sync),
    ) -> Result<(), BurnError> {
        #[cfg(target_os = "linux")]
        {
            if !files_dir.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    files_dir.display()
                )));
            }

            // Using xorriso / wodim -data
            let mut cmd = Command::new("wodim");
            cmd.args(["-v", "-data"]);
            cmd.arg(format!("dev={drive_id}"));

            if speed > 0 {
                cmd.arg(format!("speed={speed}"));
            }

            cmd.arg(files_dir);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn wodim data burn: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_wodim_progress(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for wodim data burn failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "wodim data burn failed with exit code: {status}"
                )));
            }

            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (drive_id, files_dir, speed, progress_cb);
            Err(BurnError::Unsupported(
                "Linux disc burner is only supported on Linux".into(),
            ))
        }
    }

    fn eject(&self, drive_id: &str) -> Result<(), BurnError> {
        #[cfg(target_os = "linux")]
        {
            let output = Command::new("eject").arg(drive_id).output();
            match output {
                Ok(out) if out.status.success() => Ok(()),
                _ => {
                    let dev_arg = format!("dev={drive_id}");
                    let cdr_out = Command::new("wodim")
                        .args(["-eject", &dev_arg])
                        .output()
                        .map_err(|e| {
                            BurnError::ExecutionFailed(format!("Failed to run eject / wodim: {e}"))
                        })?;
                    if !cdr_out.status.success() {
                        let err = String::from_utf8_lossy(&cdr_out.stderr);
                        return Err(BurnError::ExecutionFailed(format!("Eject failed: {err}")));
                    }
                    Ok(())
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "Linux disc burner is only supported on Linux".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROC_CDROM_INFO: &str = r#"CD-ROM information, Id: cdrom.c 3.20 2003/12/17

drive name:             sr1     sr0
drive speed:            24      48
drive # of slots:       1       1
Can close tray:         1       1
Can open tray:          1       1
Can lock tray:          1       1
Can change speed:       1       1
Can select disk:        0       0
Can read multisession:  1       1
Can read MCN:           1       1
Reports media changed:  1       1
Can play audio:         1       1
Can write CD-R:         1       1
Can write CD-RW:        1       1
Can read DVD:           1       1
Can write DVD-R:        1       1
Can write DVD-RAM:      0       0
Can read MRW:           1       1
Can write MRW:          1       1
Can write RAM:          1       1
"#;

    const SAMPLE_WODIM_DEVICES: &str = r#"wodim: Overview of accessible drives (2 found) :
-------------------------------------------------------------------------
 0  dev='/dev/sr0'	rwrw-- : 'HL-DT-ST' 'BD-RE WH16NS40'
 1  dev='/dev/sr1'	rwrw-- : 'ASUS' 'BW-16D1HT'
-------------------------------------------------------------------------
"#;

    const SAMPLE_WODIM_PRCAP_BLANK: &str = r#"Device type    : Removable CD-ROM
Version        : 0
Response Format: 2
Capabilities   :
  Does read CD-R media
  Does write CD-R media
Current: CD-R
Mounted media type: CD-R
Disk status: blank
Free Blocks: 360000
"#;

    const SAMPLE_WODIM_PRCAP_NO_MEDIA: &str = r#"wodim: Cannot load media.
wodim: No disk / Wrong disk.
"#;

    #[test]
    fn test_parse_proc_cdrom_info() {
        let drives = parse_proc_cdrom_info(SAMPLE_PROC_CDROM_INFO);
        assert_eq!(drives.len(), 2);
        assert_eq!(drives[0].id, "/dev/sr0");
        assert_eq!(drives[1].id, "/dev/sr1");
    }

    #[test]
    fn test_parse_wodim_devices() {
        let drives = parse_wodim_devices(SAMPLE_WODIM_DEVICES);
        assert_eq!(drives.len(), 2);
        assert_eq!(drives[0].id, "/dev/sr0");
        assert_eq!(drives[0].vendor, "HL-DT-ST");
        assert_eq!(drives[0].product, "BD-RE WH16NS40");

        assert_eq!(drives[1].id, "/dev/sr1");
        assert_eq!(drives[1].vendor, "ASUS");
        assert_eq!(drives[1].product, "BW-16D1HT");
    }

    #[test]
    fn test_parse_wodim_media_status_blank() {
        let status = parse_wodim_media_status(SAMPLE_WODIM_PRCAP_BLANK, "/dev/sr0");
        assert_eq!(status.drive_id, "/dev/sr0");
        assert!(status.media_present);
        assert!(status.is_blank);
        assert_eq!(status.media_type, "CD-R");
        assert_eq!(status.free_blocks, 360_000);
        assert_eq!(status.free_minutes, 80.0);
    }

    #[test]
    fn test_parse_wodim_media_status_no_media() {
        let status = parse_wodim_media_status(SAMPLE_WODIM_PRCAP_NO_MEDIA, "/dev/sr0");
        assert_eq!(status.drive_id, "/dev/sr0");
        assert!(!status.media_present);
        assert!(!status.is_blank);
        assert_eq!(status.media_type, "None");
        assert_eq!(status.free_blocks, 0);
        assert_eq!(status.free_minutes, 0.0);
    }

    #[test]
    fn test_parse_wodim_progress() {
        let p1 = parse_wodim_progress("Starting to write CD/DVD at speed 24").unwrap();
        assert_eq!(p1.stage, "Preparing");
        assert_eq!(p1.percent, 5.0);

        let p2 = parse_wodim_progress(
            "Track 01:   12 of   48 MB written (fifo 100%) [buf  98%]   4.5x.",
        )
        .unwrap();
        assert_eq!(p2.stage, "Writing");
        assert_eq!(p2.current_track, Some(1));
        // 12 / 48 = 0.25 * 90 + 5 = 27.5
        assert!((p2.percent - 27.5).abs() < 0.1);

        let p3 = parse_wodim_progress("Fixating...").unwrap();
        assert_eq!(p3.stage, "Closing");
        assert_eq!(p3.percent, 95.0);

        let p4 = parse_wodim_progress("Writing done.").unwrap();
        assert_eq!(p4.stage, "Finished");
        assert_eq!(p4.percent, 100.0);
    }
}
