use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use super::{BurnError, BurnProgress, DiscBurner, MediaStatus, OpticalDrive};

#[derive(Debug, Default, Clone)]
pub struct MacosBurner;

impl MacosBurner {
    pub fn new() -> Self {
        Self
    }
}

/// Parses MSF (Minutes:Seconds:Frames) time string into total blocks and fractional minutes.
/// CD standard: 75 frames per second, 60 seconds per minute (4500 frames/blocks per minute).
pub fn parse_msf(msf: &str) -> Option<(u64, f64)> {
    let parts: Vec<&str> = msf.trim().split(':').collect();
    if parts.len() == 3 {
        let minutes: u64 = parts[0].parse().ok()?;
        let seconds: u64 = parts[1].parse().ok()?;
        let frames: u64 = parts[2].parse().ok()?;
        let total_blocks = minutes * 60 * 75 + seconds * 75 + frames;
        let total_minutes = minutes as f64 + (seconds as f64 / 60.0) + (frames as f64 / 4500.0);
        Some((total_blocks, total_minutes))
    } else {
        None
    }
}

/// Parses the XML output of `drutil list -xml` into a list of `OpticalDrive`.
pub fn parse_drutil_list_xml(xml: &str) -> Result<Vec<OpticalDrive>, BurnError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut drives = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_vendor: Option<String> = None;
    let mut current_product: Option<String> = None;
    let mut current_interconnect: Option<String> = None;
    let mut in_device = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"device" {
                    in_device = true;
                    current_id = None;
                    current_vendor = None;
                    current_product = None;
                    current_interconnect = None;
                    for attr in e.attributes() {
                        let attr = attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                        if attr.key.as_ref() == b"index" {
                            current_id = Some(String::from_utf8_lossy(&attr.value).to_string());
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if in_device {
                    match tag {
                        b"vendor" => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"name" {
                                    current_vendor = Some(
                                        String::from_utf8_lossy(&attr.value).trim().to_string(),
                                    );
                                }
                            }
                        }
                        b"product" => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"name" {
                                    current_product = Some(
                                        String::from_utf8_lossy(&attr.value).trim().to_string(),
                                    );
                                }
                            }
                        }
                        b"interconnect" => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"name" {
                                    current_interconnect = Some(
                                        String::from_utf8_lossy(&attr.value).trim().to_string(),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"device" {
                    in_device = false;
                    let id = current_id
                        .take()
                        .unwrap_or_else(|| (drives.len() + 1).to_string());
                    let vendor = current_vendor.take().unwrap_or_default();
                    let product = current_product.take().unwrap_or_default();
                    let interconnect = current_interconnect
                        .take()
                        .unwrap_or_else(|| "Unknown".into());

                    drives.push(OpticalDrive {
                        id,
                        vendor,
                        product,
                        interconnect,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(BurnError::ParseError(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(drives)
}

/// Parses the XML output of `drutil status -xml` for a specific drive ID.
pub fn parse_drutil_status_xml(xml: &str, target_drive_id: &str) -> Result<MediaStatus, BurnError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    let mut current_device_id: Option<String> = None;
    let mut in_target_device = false;
    let mut in_status = false;
    let mut in_media_info = false;

    let mut media_present = false;
    let mut is_blank = false;
    let mut media_type: Option<String> = None;
    let mut free_blocks: u64 = 0;
    let mut free_minutes: f64 = 0.0;

    let target_normalized = target_drive_id.trim();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let tag = name.as_ref();
                match tag {
                    b"statusfordevice" => {
                        in_status = true;
                        current_device_id = None;
                        media_present = false;
                        is_blank = false;
                        media_type = None;
                        free_blocks = 0;
                        free_minutes = 0.0;
                    }
                    b"device" => {
                        for attr in e.attributes() {
                            let attr =
                                attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                            if attr.key.as_ref() == b"index" {
                                let id = String::from_utf8_lossy(&attr.value).to_string();
                                if target_normalized.is_empty() || id == target_normalized {
                                    in_target_device = true;
                                }
                                current_device_id = Some(id);
                            }
                        }
                    }
                    b"mediaInfo" if in_target_device => {
                        in_media_info = true;
                        media_present = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if in_target_device {
                    match tag {
                        b"mediaIsPresent" => {
                            media_present = true;
                        }
                        b"blank" => {
                            if in_media_info {
                                is_blank = true;
                            }
                        }
                        b"freeSpace" => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"msf" {
                                    let msf_str = String::from_utf8_lossy(&attr.value);
                                    if let Some((blocks, minutes)) = parse_msf(&msf_str) {
                                        free_blocks = blocks;
                                        free_minutes = minutes;
                                    }
                                }
                            }
                        }
                        b"mediaType" => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"value" {
                                    media_type =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                        b"mediaClass" if media_type.is_none() => {
                            for attr in e.attributes() {
                                let attr =
                                    attr.map_err(|err| BurnError::ParseError(err.to_string()))?;
                                if attr.key.as_ref() == b"value" {
                                    media_type =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let tag = name.as_ref();
                match tag {
                    b"mediaInfo" => {
                        in_media_info = false;
                    }
                    b"statusfordevice" => {
                        if in_target_device {
                            let drive_id =
                                current_device_id.unwrap_or_else(|| target_drive_id.to_string());
                            return Ok(MediaStatus {
                                drive_id,
                                media_present,
                                is_blank,
                                media_type: media_type.unwrap_or_else(|| {
                                    if media_present {
                                        "CD-R".into()
                                    } else {
                                        "None".into()
                                    }
                                }),
                                free_blocks,
                                free_minutes: (free_minutes * 100.0).round() / 100.0,
                            });
                        }
                        in_status = false;
                        in_target_device = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(BurnError::ParseError(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    if in_status && in_target_device {
        let drive_id = current_device_id.unwrap_or_else(|| target_drive_id.to_string());
        return Ok(MediaStatus {
            drive_id,
            media_present,
            is_blank,
            media_type: media_type.unwrap_or_else(|| {
                if media_present {
                    "CD-R".into()
                } else {
                    "None".into()
                }
            }),
            free_blocks,
            free_minutes: (free_minutes * 100.0).round() / 100.0,
        });
    }

    Err(BurnError::DriveNotFound(target_drive_id.to_string()))
}

/// Parses a line of `drutil burn` console output to extract stage, percentage, and track numbers.
pub fn parse_drutil_progress(line: &str) -> Option<BurnProgress> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();

    if lower.contains("preparing") || lower.contains("initiating") {
        Some(BurnProgress {
            stage: "Preparing".into(),
            percent: 5.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.contains("closing") || lower.contains("finishing") || lower.contains("flushing")
    {
        Some(BurnProgress {
            stage: "Closing".into(),
            percent: 95.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.contains("complete") || lower.contains("success") || lower.contains("finished")
    {
        Some(BurnProgress {
            stage: "Finished".into(),
            percent: 100.0,
            current_track: None,
            total_tracks: None,
            message: trimmed.to_string(),
        })
    } else if lower.contains("writing") || lower.contains("burning") || lower.contains("track") {
        // Attempt to extract track numbers e.g. "track 2 of 10"
        let mut curr_track = None;
        let mut tot_tracks = None;

        if let Some(track_pos) = lower.find("track") {
            let rest = &lower[track_pos + 5..];
            let tokens: Vec<&str> = rest.split_whitespace().collect();
            if let Some(t) = tokens.first() {
                if let Ok(num) = t.parse::<u32>() {
                    curr_track = Some(num);
                }
            }
            if let Some(of_pos) = tokens.iter().position(|&r| r == "of") {
                if let Some(total_str) = tokens.get(of_pos + 1) {
                    let cleaned = total_str.trim_matches(|c: char| !c.is_ascii_digit());
                    if let Ok(num) = cleaned.parse::<u32>() {
                        tot_tracks = Some(num);
                    }
                }
            }
        }

        // Check for percentage e.g. "45%"
        let percent = if let Some(pct_pos) = trimmed.find('%') {
            let start = trimmed[..pct_pos]
                .rfind(|c: char| !c.is_ascii_digit() && c != '.')
                .map(|i| i + 1)
                .unwrap_or(0);
            trimmed[start..pct_pos]
                .trim()
                .parse::<f32>()
                .unwrap_or(50.0)
        } else if let (Some(cur), Some(tot)) = (curr_track, tot_tracks) {
            if tot > 0 {
                (cur as f32 / tot as f32) * 85.0 + 5.0
            } else {
                50.0
            }
        } else {
            50.0
        };

        Some(BurnProgress {
            stage: "Writing".into(),
            percent,
            current_track: curr_track,
            total_tracks: tot_tracks,
            message: trimmed.to_string(),
        })
    } else {
        None
    }
}

impl DiscBurner for MacosBurner {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError> {
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("drutil")
                .args(["list", "-xml"])
                .output()
                .map_err(|e| BurnError::ExecutionFailed(format!("Failed to run drutil: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!(
                    "drutil list failed: {stderr}"
                )));
            }

            let xml = String::from_utf8_lossy(&output.stdout);
            parse_drutil_list_xml(&xml)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(BurnError::Unsupported(
                "drutil is only available on macOS".into(),
            ))
        }
    }

    fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError> {
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("drutil")
                .args(["status", "-xml"])
                .output()
                .map_err(|e| BurnError::ExecutionFailed(format!("Failed to run drutil: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!(
                    "drutil status failed: {stderr}"
                )));
            }

            let xml = String::from_utf8_lossy(&output.stdout);
            parse_drutil_status_xml(&xml, drive_id)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "drutil is only available on macOS".into(),
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
        #[cfg(target_os = "macos")]
        {
            if !tracks_or_cue.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    tracks_or_cue.display()
                )));
            }

            let mut cmd = Command::new("drutil");
            cmd.arg("-drive").arg(drive_id).arg("burn").arg("-audio");

            if speed > 0 {
                cmd.arg("-speed").arg(speed.to_string());
            }

            cmd.arg(tracks_or_cue);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            progress_cb(BurnProgress {
                stage: "Preparing".into(),
                percent: 0.0,
                current_track: None,
                total_tracks: None,
                message: "Starting drutil burn -audio...".into(),
            });

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn drutil burn: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_drutil_progress(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for drutil failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "drutil burn exited with status {status}"
                )));
            }

            progress_cb(BurnProgress {
                stage: "Finished".into(),
                percent: 100.0,
                current_track: None,
                total_tracks: None,
                message: "Burn finished successfully".into(),
            });

            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (drive_id, tracks_or_cue, speed, progress_cb);
            Err(BurnError::Unsupported(
                "drutil is only available on macOS".into(),
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
        #[cfg(target_os = "macos")]
        {
            if !files_dir.exists() {
                return Err(BurnError::IoError(format!(
                    "Path does not exist: {}",
                    files_dir.display()
                )));
            }

            let mut cmd = Command::new("drutil");
            cmd.arg("-drive").arg(drive_id).arg("burn");

            if speed > 0 {
                cmd.arg("-speed").arg(speed.to_string());
            }

            cmd.arg(files_dir);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            progress_cb(BurnProgress {
                stage: "Preparing".into(),
                percent: 0.0,
                current_track: None,
                total_tracks: None,
                message: "Starting drutil burn (data)...".into(),
            });

            let mut child = cmd.spawn().map_err(|e| {
                BurnError::ExecutionFailed(format!("Failed to spawn drutil burn: {e}"))
            })?;

            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Some(prog) = parse_drutil_progress(&line) {
                        progress_cb(prog);
                    }
                }
            }

            let status = child.wait().map_err(|e| {
                BurnError::ExecutionFailed(format!("Waiting for drutil failed: {e}"))
            })?;

            if !status.success() {
                return Err(BurnError::ExecutionFailed(format!(
                    "drutil burn exited with status {status}"
                )));
            }

            progress_cb(BurnProgress {
                stage: "Finished".into(),
                percent: 100.0,
                current_track: None,
                total_tracks: None,
                message: "Data burn finished successfully".into(),
            });

            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (drive_id, files_dir, speed, progress_cb);
            Err(BurnError::Unsupported(
                "drutil is only available on macOS".into(),
            ))
        }
    }

    fn eject(&self, drive_id: &str) -> Result<(), BurnError> {
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("drutil")
                .args(["-drive", drive_id, "tray", "eject"])
                .output()
                .map_err(|e| {
                    BurnError::ExecutionFailed(format!("Failed to run drutil eject: {e}"))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(BurnError::ExecutionFailed(format!(
                    "drutil eject failed: {stderr}"
                )));
            }

            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = drive_id;
            Err(BurnError::Unsupported(
                "drutil is only available on macOS".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LIST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!DOCTYPE  [
    <!ELEMENT deviceList (device)*>
    <!ELEMENT device (vendor, product, firmware, interconnect, support)>
    <!ATTLIST device index CDATA #IMPLIED>
    <!ELEMENT vendor EMPTY>
    <!ATTLIST vendor name CDATA #REQUIRED>
    <!ELEMENT product EMPTY>
    <!ATTLIST product name CDATA #REQUIRED>
    <!ELEMENT firmware EMPTY>
    <!ATTLIST firmware revision CDATA #REQUIRED>
    <!ELEMENT interconnect EMPTY>
    <!ATTLIST interconnect name CDATA #REQUIRED>
    <!ELEMENT support EMPTY>
    <!ATTLIST support level (appleShipping|appleSupported|vendorSupported|unSupported|notSupported) #REQUIRED>
]>

<deviceList>
    <device index="1">
        <vendor name="HL-DT-ST"/>
        <product name="BD-RE  WH16NS40"/>
        <firmware revision="1.05"/>
        <interconnect name="USB"/>
        <support level="appleSupported"/>
    </device>
    <device index="2">
        <vendor name="ASUS"/>
        <product name="BW-16D1HT"/>
        <firmware revision="3.10"/>
        <interconnect name="SATA"/>
        <support level="vendorSupported"/>
    </device>
</deviceList>
"#;

    const SAMPLE_STATUS_BLANK_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<statusdoc>
    <statusfordevice>
        <device index="1">
            <vendor name="HL-DT-ST"/>
            <product name="BD-RE  WH16NS40"/>
            <firmware revision="1.05"/>
            <interconnect name="USB"/>
            <support level="appleSupported"/>
        </device>
        <deviceStatus>
            <writeSpeed currentSpeed="0"/>
            <mediaIsPresent/>
            <mediaInfo>
                <blank/>
                <usedSpace msf="00:00:00"/>
                <freeSpace msf="79:59:74"/>
                <overwritableSpace msf="00:00:00"/>
                <mediaClass value="CD"/>
                <mediaType value="CD-R"/>
            </mediaInfo>
            <trayState value="closed"/>
        </deviceStatus>
    </statusfordevice>
</statusdoc>
"#;

    const SAMPLE_STATUS_NO_MEDIA_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<statusdoc>
    <statusfordevice>
        <device index="1">
            <vendor name="HL-DT-ST"/>
            <product name="BD-RE  WH16NS40"/>
            <firmware revision="1.05"/>
            <interconnect name="USB"/>
            <support level="appleSupported"/>
        </device>
        <deviceStatus>
            <writeSpeed currentSpeed="0"/>
            <trayState value="open"/>
        </deviceStatus>
    </statusfordevice>
</statusdoc>
"#;

    #[test]
    fn test_parse_msf() {
        let (blocks, minutes) = parse_msf("79:59:74").unwrap();
        // 79 * 4500 + 59 * 75 + 74 = 355500 + 4425 + 74 = 359999
        assert_eq!(blocks, 359999);
        assert!((minutes - 80.0).abs() < 0.01);

        let (blocks_zero, minutes_zero) = parse_msf("00:00:00").unwrap();
        assert_eq!(blocks_zero, 0);
        assert_eq!(minutes_zero, 0.0);
    }

    #[test]
    fn test_parse_drutil_list_xml() {
        let drives = parse_drutil_list_xml(SAMPLE_LIST_XML).unwrap();
        assert_eq!(drives.len(), 2);

        assert_eq!(drives[0].id, "1");
        assert_eq!(drives[0].vendor, "HL-DT-ST");
        assert_eq!(drives[0].product, "BD-RE  WH16NS40");
        assert_eq!(drives[0].interconnect, "USB");

        assert_eq!(drives[1].id, "2");
        assert_eq!(drives[1].vendor, "ASUS");
        assert_eq!(drives[1].product, "BW-16D1HT");
        assert_eq!(drives[1].interconnect, "SATA");
    }

    #[test]
    fn test_parse_drutil_empty_list_xml() {
        let empty_xml = "<deviceList/>";
        let drives = parse_drutil_list_xml(empty_xml).unwrap();
        assert!(drives.is_empty());
    }

    #[test]
    fn test_parse_drutil_status_blank_media() {
        let status = parse_drutil_status_xml(SAMPLE_STATUS_BLANK_XML, "1").unwrap();
        assert_eq!(status.drive_id, "1");
        assert!(status.media_present);
        assert!(status.is_blank);
        assert_eq!(status.media_type, "CD-R");
        assert_eq!(status.free_blocks, 359999);
        assert_eq!(status.free_minutes, 80.0);
    }

    #[test]
    fn test_parse_drutil_status_no_media() {
        let status = parse_drutil_status_xml(SAMPLE_STATUS_NO_MEDIA_XML, "1").unwrap();
        assert_eq!(status.drive_id, "1");
        assert!(!status.media_present);
        assert!(!status.is_blank);
        assert_eq!(status.media_type, "None");
        assert_eq!(status.free_blocks, 0);
        assert_eq!(status.free_minutes, 0.0);
    }

    #[test]
    fn test_parse_drutil_status_not_found() {
        let err = parse_drutil_status_xml(SAMPLE_STATUS_BLANK_XML, "99").unwrap_err();
        assert!(matches!(err, BurnError::DriveNotFound(_)));
    }

    #[test]
    fn test_parse_drutil_progress() {
        let p1 = parse_drutil_progress("Preparing burn...").unwrap();
        assert_eq!(p1.stage, "Preparing");

        let p2 = parse_drutil_progress("Writing Track 3 of 12 (25%)...").unwrap();
        assert_eq!(p2.stage, "Writing");
        assert_eq!(p2.current_track, Some(3));
        assert_eq!(p2.total_tracks, Some(12));
        assert_eq!(p2.percent, 25.0);

        let p3 = parse_drutil_progress("Closing session...").unwrap();
        assert_eq!(p3.stage, "Closing");
        assert_eq!(p3.percent, 95.0);

        let p4 = parse_drutil_progress("Burn finished successfully").unwrap();
        assert_eq!(p4.stage, "Finished");
        assert_eq!(p4.percent, 100.0);

        assert!(parse_drutil_progress("   ").is_none());
    }
}
