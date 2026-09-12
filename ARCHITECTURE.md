# SpotyBurn Architecture & Technical Decision Document

This document provides a comprehensive technical overview of the architecture, design principles, and engineering decisions underlying **SpotyBurn**, a modern cross-platform desktop application designed to convert Spotify playlists and albums into standard-compliant Red Book Audio CDs and MP3 Data CDs.

---

## 1. Technology Stack Decision & Evaluation

### 1.1 Evaluated Frameworks & Runtimes

When building a high-performance cross-platform desktop application interfacing directly with low-level optical hardware, audio transcoders, and external network APIs, four primary stacks were evaluated:

| Criterion | **Rust + Tauri v2** *(Selected)* | **Electron** | **Go + Wails v2** | **Python (PyQt / Tkinter)** |
| :--- | :--- | :--- | :--- | :--- |
| **Idle Memory (RAM)** | **~25 – 40 MB** | ~120 – 250 MB | ~50 – 80 MB | ~70 – 140 MB |
| **Installer / Binary Size** | **~8 – 20 MB** | ~85 – 140 MB | ~15 – 35 MB | ~60 – 120 MB |
| **Runtime Engine** | Native OS WebView (WebKit / WebView2) | Bundled Chromium & Node.js | Native OS WebView | Python Interpreter + Qt C++ |
| **GC / Buffer Stability** | **Zero GC** (Deterministic) | V8 GC Pauses (Risk of buffer underrun) | Go GC Pauses (Risk of buffer underrun) | GIL + Python GC |
| **Low-Level Hardware Access** | Direct C-FFI, COM, POSIX `ioctl` | Node-gyp native addons required | CGo required (cross-compile friction) | PyWin32 / ctypes |
| **Type Safety & Concurrency** | Compile-time memory safety, Tokio async | Async JS (Single-threaded event loop) | Goroutines (Good, but GC pauses) | Multi-threading constrained by GIL |
| **Security Surface** | Strict CSP, sandboxed IPC, minimal binary | Huge Chromium attack surface | Small footprint, but weaker IPC guards | PyInstaller unpackable to raw code |

### 1.2 Rationale for Rust + Tauri v2

1. **Deterministic Execution Without Garbage Collection:**
   Writing an optical Audio CD requires streaming 75 sectors (176,400 bytes) of PCM audio per second. Any prolonged garbage collection stop-the-world pause or thread starvation on older or external USB optical writers risks **buffer under-run**, resulting in an unrecoverable coaster (ruined CD-R). Rust provides zero-cost abstractions, deterministic destructors, and memory control without garbage collection.

2. **Native Operating System Interoperability:**
   SpotyBurn must speak native operating system protocols:
   - On Windows: Component Object Model (COM) and IMAPI2 (`IDiscMaster2`, `IDiscFormat2TrackAtOnce`).
   - On macOS: DiscRecording framework and `drutil`.
   - On Linux: POSIX `ioctl`, `/proc/sys/dev/cdrom/info`, and optical utilities (`wodim`, `xorriso`).
   Rust interacts seamlessly with OS primitives via standard C-ABI, FFI, and native process pipes without runtime marshaling penalties.

3. **Ultra-Lean Resource Footprint:**
   While Electron bundles a 100+ MB Chromium runtime and consumes 200+ MB RAM to display a dashboard, Tauri v2 utilizes the operating system's built-in web engine (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux). The resulting SpotyBurn binary is typically under 15 MB, launches instantly, and idles at ~30 MB RAM.

4. **Robust Frontend Decoupling:**
   The frontend is implemented in standard HTML5, CSS3, and ES6+ JavaScript without reliance on heavy frameworks (React/Vue/Angular). It communicates with the Rust backend via Tauri's IPC invoke commands and strongly typed event streams (`burn-progress`, `burn-log`, `burn-finished`, `burn-error`).

---

## 2. Red Book Audio-CD Specification (CD-DA)

Standard optical CD audio is governed by **IEC 60908** (commonly known as the **Red Book** standard). Standard standalone CD players (including older home stereo decks, discmans, and car CD changers) strictly require compliance with these physics and framing parameters.

```
+-------------------------------------------------------------------------+
|                       Red Book Audio Frame / Sector                      |
|                           (1/75th of a second)                          |
+-------------------------------------------------------------------------+
| 44,100 Hz Sample Rate ÷ 75 Sectors/s = 588 Samples / Sector             |
| 588 Samples × 2 Channels (Stereo) × 2 Bytes (16-bit) = 2,352 Audio Bytes|
+-------------------------------------------------------------------------+
```

### 2.1 Audio Encoding Parameters

- **Sampling Frequency:** Exactly `44,100 Hz` (44.1 kHz).
- **Bit Depth:** `16-bit` signed integer linear PCM (Little-Endian byte order).
- **Channels:** `2` channels (Stereo: Left and Right interleaved).
- **Bitrate:** `44,100 samples/s × 16 bits/sample × 2 channels = 1,411,200 bits/s` (1,411.2 kbps).

### 2.2 Sector Alignment & Padding (The 2,352-Byte Rule)

- An optical Audio CD is divided into physical sectors or "frames". Each frame represents exactly **1/75th of a second** (13.33 milliseconds) of audio.
- One sector contains exactly:
  $$\text{Bytes per Sector} = 44,100 \times \frac{1}{75} \times 2 \times 2 = 2,352\text{ bytes}$$
- **The Padding Requirement:** The audio payload (`data` chunk) of every WAV file written to a Red Book CD **must be an exact integer multiple of 2,352 bytes**.
- If a WAV file's audio length is not aligned to 2,352 bytes:
  - Many hardware burners and drivers (IMAPI2, drutil, wodim) reject the track with write errors.
  - Software that forcibly truncates or pads incorrectly causes an audible popping/clicking glitch or invalid lead-out indexing.
- **SpotyBurn Implementation (`audio.rs`):**
  - `calculate_sector_padding(data_len)` calculates $P = (2352 - (\text{data\_len} \pmod{2352})) \pmod{2352}$.
  - `pad_wav_buffer` and `pad_wav_file` append $P$ zero bytes (silence) to the PCM stream and rewrite both the `RIFF` chunk size and `data` subchunk header in place.

### 2.3 Loudness Normalization (EBU R128 & ITU-R BS.1770-4)

Audio sourced from modern streaming services or varied releases features vastly disparate dynamic ranges and perceived loudness levels (ranging from -24 LUFS for acoustic/classical recordings to -6 LUFS for modern hyper-compressed pop/EDM). Burning these unnormalized onto a single CD forces the listener to constantly adjust volume.

SpotyBurn integrates **EBU R128** two-stage loudness normalization through FFmpeg:
- **Integrated Loudness Target:** `-16 LUFS` (optimal target for domestic CD-DA listening and high-fidelity car stereos).
- **True Peak Ceiling:** `-1.0 dBTP` (prevents inter-sample peaks from clipping during digital-to-analog reconstruction in DAC filters).
- **Loudness Range (LRA):** Target `11 LU`.
- **FFmpeg Filtergraph:**
  ```bash
  -af "loudnorm=I=-16:TP=-1.0:LRA=11,aformat=sample_fmts=s16:sample_rates=44100:channel_layouts=stereo"
  ```

### 2.4 CUE Sheets & CD-Text

- **CUE Sheet Architecture:**
  SpotyBurn generates industry-standard CDRWIN-compatible CUE sheets (`cuesheet.rs`) representing the Disc-At-Once (DAO) master layout:
  ```cue
  PERFORMER "Various Artists"
  TITLE "Road Trip 2026"
  FILE "track_01.wav" WAVE
    TRACK 01 AUDIO
      TITLE "Blinding Lights"
      PERFORMER "The Weeknd"
      PREGAP 00:02:00
      INDEX 01 00:00:00
  FILE "track_02.wav" WAVE
    TRACK 02 AUDIO
      TITLE "Levitating"
      PERFORMER "Dua Lipa"
      INDEX 01 00:00:00
  ```
- **Standard 2-Second Pregap:**
  The Red Book standard mandates a 2-second pause (150 sectors) before Track 1 (`INDEX 00 00:00:00` to `INDEX 01 00:02:00`). SpotyBurn automatically injects this pregap in the generated CUE sheet.
- **CD-Text:**
  Track metadata (`TITLE`, `PERFORMER`, `ISRC`) is embedded into the subchannel R to W areas of the disc lead-in, enabling CD-Text compatible car stereos and standalone players to display title and artist information during playback.

---

## 3. Cross-Platform Optical Burning Engine

SpotyBurn isolates platform-specific hardware burning logic behind a unified Rust trait (`DiscBurner` in `burner/mod.rs`):

```rust
pub trait DiscBurner: Send + Sync {
    fn detect_drives(&self) -> Result<Vec<OpticalDrive>, BurnError>;
    fn get_media_status(&self, drive_id: &str) -> Result<MediaStatus, BurnError>;
    fn burn_audio_cd(&self, options: &BurnOptions, cb: Option<ProgressCallback>) -> Result<(), BurnError>;
    fn burn_data_cd(&self, options: &BurnOptions, cb: Option<ProgressCallback>) -> Result<(), BurnError>;
    fn eject(&self, drive_id: &str) -> Result<(), BurnError>;
}
```

```
                     +---------------------------------------+
                     |           SpotyBurn Frontend          |
                     +---------------------------------------+
                                         |
                            Tauri IPC Invoke / Events
                                         v
                     +---------------------------------------+
                     |         Audio / Burn Pipeline         |
                     +---------------------------------------+
                                         |
                                 DiscBurner Trait
                                         |
             +---------------------------+---------------------------+
             |                           |                           |
             v                           v                           v
     +---------------+           +---------------+           +---------------+
     |  macOS Engine |           | Windows Engine|           |  Linux Engine |
     | (burner/macos)|           |(burner/windows|           | (burner/linux)|
     +---------------+           +---------------+           +---------------+
             |                           |                           |
        drutil /                    IMAPI2 COM /               wodim / xorriso
     DiscRecording                  PowerShell                  POSIX ioctl
```

### 3.1 Windows Subsystem: IMAPI2 (Image Mastering API v2)

- **Architecture:** Windows provides IMAPI2 through COM (Component Object Model) interfaces registered in `imapi2.dll` and `imapi2fs.dll`.
- **Core Interfaces Utilized:**
  - `IDiscMaster2`: Optical drive enumeration, device addition/removal notifications.
  - `IDiscRecorder2`: Drive hardware locking, tray ejection, device identification.
  - `IDiscFormat2TrackAtOnce`: Red Book Audio-CD track-at-once mastering and raw audio streaming.
  - `IDiscFormat2Data`: File system burning (ISO 9660 / Joliet / UDF) for Data CD mode.
- **Robust Execution Strategy:**
  To guarantee compatibility across all modern Windows installations (Windows 10, Windows 11 on x86_64 and ARM64) without requiring manual Visual Studio C++ runtime DLL redistribution, SpotyBurn provides:
  1. A native Rust COM layer.
  2. A dedicated PowerShell COM bridge script (`src-tauri/scripts/burn_imapi2.ps1`), invoked directly with JSON input/output and event streaming, translating IMAPI2 COM events (`DDiscFormat2TrackAtOnceEvents`) into real-time progress callbacks.

### 3.2 macOS Subsystem: `drutil` & DiscRecording

- **Architecture:** macOS manages optical burning via the native `DiscRecording.framework`. The command-line utility `drutil` provides an interface to this engine.
- **Drive Discovery & Media Status:**
  - SpotyBurn invokes `drutil list -xml` and `drutil status -xml`.
  - The XML output is parsed using high-speed streaming zero-copy deserialization (`quick-xml`).
  - Extracted metrics include vendor, product ID, interconnect (USB, SATA), media type (`CD-R`, `CD-RW`), blank status, and available free blocks/minutes.
- **Burning Pipeline:**
  - For Red Book Audio CDs: `drutil burn -audio <tracks...>`
  - Disc-At-Once (DAO) / Track-At-Once (TAO) flags configured based on drive capabilities.
  - Stdout progress stream is monitored in real-time (`Burned: XX%`) and emitted to the UI via Tauri events.

### 3.3 Linux Subsystem: `wodim`, `xorriso`, and POSIX `cdrom`

- **Architecture:** Linux provides optical hardware access through the kernel's SCSI/IDE generic subsystem (`/dev/sr*`, `/dev/cdrom`).
- **Drive Discovery & Media Status:**
  - System drives are enumerated by querying `/proc/sys/dev/cdrom/info` and `wodim --devices`.
  - Disc media status is retrieved via `wodim -prcap dev=<device>` or `xorriso -out_dev <device> -toc`.
- **Burning Pipeline:**
  - Audio CD: `wodim -v -dao -pad -audio speed=<speed> dev=<device> <wav_files...>`
  - Data CD: `xorriso -as mkisofs -r -J <files...> | wodim -v -data speed=<speed> dev=<device> -`

---

## 4. Dependency Isolation & Sidecar Strategy

A core design requirement of SpotyBurn is **zero manual user terminal setup**. Non-technical desktop users must never be forced to install Homebrew, configure environment PATHs, or compile binary dependencies.

### 4.1 Four-Layer Binary Resolution Hierarchy

Both the `AudioPipeline` and `Downloader` resolve required binaries (`ffmpeg` and `yt-dlp`) using a strict 4-layer resolution strategy:

```
[1. Explicit Hint / Custom Path (Settings)]
                   │
                   ▼ (if not found)
[2. Environment Override (SPOTYBURN_FFMPEG_PATH / SPOTYBURN_YT_DLP_PATH)]
                   │
                   ▼ (if not found)
[3. Application Sandbox (~/.spotyburn/bin/)]
                   │
                   ▼ (if not found)
[4. System PATH & Standard Locations (/opt/homebrew/bin, /usr/bin, etc.)]
```

### 4.2 Application Sandbox (`~/.spotyburn/bin/`)

- On initial startup, if system binaries are not present, SpotyBurn can download or stage standalone static builds directly into `~/.spotyburn/bin/`.
- This location is user-writable, requiring no elevated root or administrator privileges.
- Permissions on Unix systems are explicitly set to `0755` executable mode.

### 4.3 Bundled Tauri Sidecars for Releases

- During release packaging, platform-specific static binaries are bundled alongside the application executable inside the platform app bundle (`.app/Contents/MacOS/` on macOS, application directory on Windows/Linux).
- `yt-dlp` updates can be fetched independently into the sandbox directory to guard against upstream YouTube extractor changes without requiring a full desktop app update.

---

## 5. Security & Credentials Architecture

### 5.1 Local Credential Storage

- Spotify API credentials (`client_id` and `client_secret`) are stored in the user's home configuration directory:
  - macOS/Linux: `~/.spotyburn/config.json`
  - Windows: `%USERPROFILE%\.spotyburn\config.json`
- **File System Permissions:**
  - On Unix systems, configuration files are created with mode `0600` (`S_IRUSR | S_IWUSR`), strictly restricting access to the operating user account.
- **Client Credentials OAuth2 Flow:**
  - SpotyBurn uses Spotify's official Client Credentials Grant.
  - Access tokens are kept in-memory with automatic expiration tracking and cached renewal; tokens are never persisted in plaintext logs.

### 5.2 Tauri IPC & Frontend Sandboxing

- **Strict Content Security Policy (CSP):**
  ```
  default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; connect-src 'self' https:;
  ```
- **Origin Isolation:**
  - The frontend runs in a sandboxed origin without direct Node.js or OS shell access.
  - All communication with the operating system is strictly mediated through explicit Tauri `#[tauri::command]` functions with strong input validation.
- **Shell Injection Defenses:**
  - External process invocations (`std::process::Command`) use discrete argument arrays (`.arg(...)`) rather than shell string interpolation, eliminating argument splitting and command injection vulnerabilities.

### 5.3 Temporary Storage & Cache Lifecycle

- Audio conversions and scratch files are written to job-specific subdirectories under `~/.spotyburn/cache/job_<timestamp>/`.
- Upon burn completion or user cancellation, temporary WAV files and intermediate downloads are purged to maintain user disk space.

---

## 6. Continuous Integration & Release Packaging

SpotyBurn provides an automated GitHub Actions multi-platform release workflow (`.github/workflows/release.yml`):

- **macOS:** Produces signed Universal `.dmg` disk images and `.app` bundles supporting both Apple Silicon (ARM64) and Intel (x86_64).
- **Windows:** Generates `.msi` installers and standalone `.exe` binaries with WiX/NSIS bundler integration.
- **Linux:** Generates portable `.AppImage` packages and `.deb` distribution packages for Debian/Ubuntu environments.
- **Artifact Verification:** Every build automatically calculates and publishes SHA256 checksums (`SHA256SUMS.txt`) to the GitHub Releases portal.
