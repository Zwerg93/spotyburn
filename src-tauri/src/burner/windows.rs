#[cfg(target_os = "windows")]
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::{BurnError, BurnProgress, DiscBurner, MediaStatus, OpticalDrive};

#[derive(Debug, Default, Clone)]
pub struct WindowsBurner {
    pub script_path: Option<PathBuf>,
}

impl WindowsBurner {
    pub fn new() -> Self {
        Self { script_path: None }
    }

    pub fn with_script_path(path: PathBuf) -> Self {
        Self {
            script_path: Some(path),
        }
    }

    /// Locates the `burn_imapi2.ps1` script on the filesystem.
    pub fn resolve_script_path(&self) -> Result<PathBuf, BurnError> {
        if let Some(ref path) = self.script_path {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        // Candidates: relative to cwd, relative to executable, or in scripts directory
        let candidates = [
            PathBuf::from("scripts/burn_imapi2.ps1"),
            PathBuf::from("src-tauri/scripts/burn_imapi2.ps1"),
            PathBuf::from("../scripts/burn_imapi2.ps1"),
            PathBuf::from("../../scripts/burn_imapi2.ps1"),
        ];

        for c in &candidates {
            if c.exists() {
                return Ok(c.clone());
            }
        }

        if let Ok(current_exe) = std::env::current_exe() {
            if let Some(dir) = current_exe.parent() {
                let candidate = dir.join("scripts").join("burn_imapi2.ps1");
                if candidate.exists() {
                    return Ok(candidate);
                }
                let candidate2 = dir.join("burn_imapi2.ps1");
                if candidate2.exists() {
                    return Ok(candidate2);
                }
            }
        }

        // Default fallback to expected path
        Ok(PathBuf::from("scripts/burn_imapi2.ps1"))
    }

    #[cfg(target_os = "windows")]
    fn run_ps_action(&self, args: &[&str]) -> Result<std::process::Output, BurnError> {
        let script = self.resolve_script_path()?;
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .args(args);

        cmd.output()
            .map_err(|e| BurnError::ExecutionFailed(format!("Failed to execute PowerShell: {e}")))
    }
}

#[derive(Debug, Deserialize)]
struct WindowsDriveEntry {
    pub id: String,
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub interconnect: Option<String>,
    #[allow(dead_code)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WindowsStatusEntry {
    pub drive_id: String,
    pub media_present: bool,
    pub is_blank: bool,
    pub media_type: Option<String>,
    pub free_blocks: u64,
    pub free_minutes: f64,
}

pub fn parse_windows_drive_list_json(json_str: &str) -> Result<Vec<OpticalDrive>, BurnError> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(Vec::new());
    }

    // PowerShell might return a single object or an array of objects
    if trimmed.starts_with('[') {
        let entries: Vec<WindowsDriveEntry> = serde_json::from_str(trimmed)
            .map_err(|e| BurnError::ParseError(format!("Failed to parse drive list: {e}")))?;
        Ok(entries
            .into_iter()
            .map(|e| OpticalDrive {
                id: e.id,
                vendor: e.vendor.unwrap_or_default(),
                product: e.product.unwrap_or_default(),
                interconnect: e.interconnect.unwrap_or_else(|| "ATAPI".into()),
            })
            .collect())
    } else {
        let entry: WindowsDriveEntry = serde_json::from_str(trimmed)
            .map_err(|e| BurnError::ParseError(format!("Failed to parse drive entry: {e}")))?;
        Ok(vec![OpticalDrive {
            id: entry.id,
            vendor: entry.vendor.unwrap_or_default(),
            product: entry.product.unwrap_or_default(),
            interconnect: entry.interconnect.unwrap_or_else(|| "ATAPI".into()),
        }])
    }
}

pub fn parse_windows_status_json(json_str: &str) -> Result<MediaStatus, BurnError> {
    let trimmed = json_str.trim();
    let entry: WindowsStatusEntry = serde_json::from_str(trimmed)
        .map_err(|e| BurnError::ParseError(format!("Failed to parse media status JSON: {e}")))?;

    Ok(MediaStatus {
        drive_id: entry.drive_id,
        media_present: entry.media_present,
        is_blank: entry.is_blank,
        media_type: entry.media_type.unwrap_or_else(|| {
            if entry.media_present {
                "CD-R".into()
            } else {
                "None".into()
            }
        }),
        free_blocks: entry.free_blocks,
        free_minutes: entry.free_minutes,
    })
}

/// Parses lines formatted as `PROGRESS:stage=Writing,percent=25.0,track=1,total=4,message=...`
pub fn parse_windows_progress_line(line: &str) -> Option<BurnProgress> {
    let trimmed = line.trim();
    if !trimmed.starts_with("PROGRESS:") {
        return None;
    }

    let payload = &trimmed[9..];
    let mut stage = "Writing".to_string();
    let mut percent = 0.0f32;
    let mut current_track = None;
    let mut total_tracks = None;
    let mut message = String::new();

    for part in payload.split(',') {
        if let Some((k, v)) = part.split_once('=') {
            match k.trim() {
                "stage" => stage = v.trim().to_string(),
                "percent" => {
                    if let Ok(p) = v.trim().parse::<f32>() {
                        percent = p;
                    }
                }
                "track" => {
                    if let Ok(t) = v.trim().parse::<u32>() {
                        current_track = Some(t);
                    }
                }
                "total" => {
                    if let Ok(tot) = v.trim().parse::<u32>() {
                        total_tracks = Some(tot);
                    }
                }
                "message" => message = v.trim().to_string(),
                _ => {}
            }
        }
    }

    Some(BurnProgress {
        stage,
        percent,
        current_track,
        total_tracks,
        message,
    })
}

impl DiscBurner for WindowsBurner {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError> {
        #[cfg(target_os = "windows")]
        {
            let output = self.run_ps_action(&["-Action", "list"])?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!(
                    "List drives failed: {err}"
                )));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_windows_drive_list_json(&stdout)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err(BurnError::Unsupported(
                "Windows IMAPI2 is only supported on Windows".into(),
            ))
        }
    }

    fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError> {
        #[cfg(target_os = "windows")]
        {
            let output = self.run_ps_action(&["-Action", "status", "-DriveId", drive_id])?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!(
                    "Get status failed: {err}"
                )));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_windows_status_json(&stdout)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "Windows IMAPI2 is only supported on Windows".into(),
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
        #[cfg(target_os = "windows")]
        {
            if !tracks_or_cue.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    tracks_or_cue.display()
                )));
            }

            let script = self.resolve_script_path()?;
            let speed_str = speed.to_string();
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .args([
                    "-Action",
                    "burn-audio",
                    "-DriveId",
                    drive_id,
                    "-SourcePath",
                    tracks_or_cue.to_string_lossy().as_ref(),
                    "-Speed",
                    &speed_str,
                ]);

            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn PowerShell burn: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_windows_progress_line(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for PowerShell failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "IMAPI2 burn failed with exit code: {status}"
                )));
            }

            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (drive_id, tracks_or_cue, speed, progress_cb);
            Err(BurnError::Unsupported(
                "Windows IMAPI2 is only supported on Windows".into(),
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
        #[cfg(target_os = "windows")]
        {
            if !files_dir.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    files_dir.display()
                )));
            }

            let script = self.resolve_script_path()?;
            let speed_str = speed.to_string();
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .args([
                    "-Action",
                    "burn-data",
                    "-DriveId",
                    drive_id,
                    "-SourcePath",
                    files_dir.to_string_lossy().as_ref(),
                    "-Speed",
                    &speed_str,
                ]);

            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn PowerShell burn-data: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_windows_progress_line(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for PowerShell failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "IMAPI2 data burn failed with exit code: {status}"
                )));
            }

            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (drive_id, files_dir, speed, progress_cb);
            Err(BurnError::Unsupported(
                "Windows IMAPI2 is only supported on Windows".into(),
            ))
        }
    }

    fn eject(&self, drive_id: &str) -> Result<(), BurnError> {
        #[cfg(target_os = "windows")]
        {
            let output = self.run_ps_action(&["-Action", "eject", "-DriveId", drive_id])?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!("Eject failed: {err}")));
            }
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "Windows IMAPI2 is only supported on Windows".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_windows_drive_list_json() {
        let json = r#"[
            {"id": "0", "vendor": "HL-DT-ST", "product": "BD-RE WH16NS40", "interconnect": "ATAPI", "path": "D:\\"},
            {"id": "1", "vendor": "ASUS", "product": "BW-16D1HT", "interconnect": "SATA", "path": "E:\\"}
        ]"#;

        let drives = parse_windows_drive_list_json(json).unwrap();
        assert_eq!(drives.len(), 2);
        assert_eq!(drives[0].id, "0");
        assert_eq!(drives[0].vendor, "HL-DT-ST");
        assert_eq!(drives[0].product, "BD-RE WH16NS40");
        assert_eq!(drives[0].interconnect, "ATAPI");

        assert_eq!(drives[1].id, "1");
        assert_eq!(drives[1].vendor, "ASUS");
    }

    #[test]
    fn test_parse_windows_single_drive_json() {
        let json = r#"{"id": "0", "vendor": "PIONEER", "product": "BD-RW BDR-209D", "interconnect": "SATA", "path": "E:\\"}"#;
        let drives = parse_windows_drive_list_json(json).unwrap();
        assert_eq!(drives.len(), 1);
        assert_eq!(drives[0].vendor, "PIONEER");
    }

    #[test]
    fn test_parse_windows_status_json() {
        let json = r#"{"drive_id":"0","media_present":true,"is_blank":true,"media_type":"CD-R","free_blocks":360000,"free_minutes":80.0}"#;
        let status = parse_windows_status_json(json).unwrap();
        assert_eq!(status.drive_id, "0");
        assert!(status.media_present);
        assert!(status.is_blank);
        assert_eq!(status.media_type, "CD-R");
        assert_eq!(status.free_blocks, 360000);
        assert_eq!(status.free_minutes, 80.0);
    }

    #[test]
    fn test_parse_windows_status_no_media() {
        let json = r#"{"drive_id":"1","media_present":false,"is_blank":false,"media_type":"None","free_blocks":0,"free_minutes":0.0}"#;
        let status = parse_windows_status_json(json).unwrap();
        assert_eq!(status.drive_id, "1");
        assert!(!status.media_present);
        assert!(!status.is_blank);
        assert_eq!(status.media_type, "None");
    }

    #[test]
    fn test_parse_windows_progress_line() {
        let line = "PROGRESS:stage=Writing,percent=55.5,track=2,total=4,message=Burning track 2";
        let prog = parse_windows_progress_line(line).unwrap();
        assert_eq!(prog.stage, "Writing");
        assert_eq!(prog.percent, 55.5);
        assert_eq!(prog.current_track, Some(2));
        assert_eq!(prog.total_tracks, Some(4));
        assert_eq!(prog.message, "Burning track 2");

        let non_prog = "Some regular powershell output line";
        assert!(parse_windows_progress_line(non_prog).is_none());
    }
}
