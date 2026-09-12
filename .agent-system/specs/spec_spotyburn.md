# Spezifikation: SpotyBurn (Cross-Platform Spotify Audio CD Desktop App)

## 1. Übersicht & Scope-Grenzen

### 1.1 Ziele (In-Scope)
- **Spotify Ingestion:** Anbindung der offiziellen Spotify Web API (OAuth2 Client Credentials Flow), Abruf von Playlists, Alben und Einzeltracks mit automatischer Pagination.
- **Audio Retrieval & Matching:** Metadaten-basierte Suche und Download von Audiospuren (via `yt-dlp`), strikter Laufzeit- und Metadaten-Abgleich zur Vermeidung von Fehlgriffen.
- **Red Book CD-DA Audio Pipeline:** Standardkonforme Transkodierung via FFmpeg:
  - 44.1 kHz, 16-Bit Stereo PCM (WAV)
  - Sektorausrichtung an 2352-Byte-Blöcken (1/75 s)
  - Loudness-Normalisierung (EBU R128)
  - Erzeugung von CUE-Sheets mit CD-Text (Performer, Title, Pregap 2s).
- **Native Optical Burning Engine:**
  - **macOS:** `drutil` Subsystem (Geräteerkennung, Medienprüfung, `drutil burn -audio`).
  - **Windows:** IMAPI2 (Windows Image Mastering API v2) COM-Integration.
  - **Linux:** `wodim` / `xorriso` / `cdrecord` POSIX-Treiber.
- **Kapazitätsprüfung:** Visuelle 80-Minuten-Grenze (Standard-CD-R) mit Warnung und Umschaltmöglichkeit auf Daten-/MP3-CD-Modus.
- **Desktop UI:** Leichtgewichtiges, modernes Dashboard (Setup/Credentials, Playlist-Import, Trackliste, Drive-Auswahl, Live-Burn-Log).

### 1.2 Nicht-Ziele (Out-of-Scope)
- Direktes Umgehen des Spotify-DRM (Widevine) – stattdessen legales Metadaten-Matching & Audio-Sourcing.
- Brennen von Video-DVDs oder Blu-rays (Fokus liegt auf Red Book Audio-CD und Daten-CD).
- Komplexe VST-Audio-Effekt-Plugins außerhalb von Standard-Pegelnormalisierung.

---

## 2. Tech-Stack & Architektur

- **Core & Runtime:** Rust + Tauri v2.
  - *Empfehlung des Architects:* **Rust + Tauri v2**.
  - *Vorteile:* Extrem geringer RAM-Bedarf (<40 MB), native Systemintegration (COM auf Windows, `drutil` auf macOS, POSIX auf Linux), Single-Binary-Packaging ohne Runtime-Abhängigkeiten, memory-safe Concurrency für paralleles Transkodieren.
- **Frontend:** Vanilla HTML5 / Modern CSS (Dark Theme) / TypeScript/ES6. Keine überladenen Frameworks nötig.
- **Externe Werkzeuge:** `ffmpeg` und `yt-dlp` (Dual-Mode: Erkennung im System-PATH oder automatischer Sidecar-Download in `~/.spotyburn/bin/`).

---

## 3. Datenmodelle & Schnittstellen

### 3.1 Datenmodelle

```rust
pub struct SpotifyTrack {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u64,
    pub track_number: u32,
    pub isrc: Option<String>,
}

pub struct OpticalDrive {
    pub id: String,          // z. B. "1", "/dev/sr0", "D:"
    pub vendor: String,
    pub product: String,
    pub interconnect: String, // USB, SATA, ATAPI
}

pub enum BurnMode {
    AudioCdRedBook, // max 80 Minuten, 44.1kHz 16-bit PCM WAV
    DataMp3Cd,      // max 700 MB, MP3 Dateien
}

pub struct MediaStatus {
    pub drive_id: String,
    pub media_present: bool,
    pub is_blank: bool,
    pub media_type: String,   // "CD-R", "CD-RW", "None"
    pub free_blocks: u64,
    pub free_minutes: f64,
}
```

### 3.2 Schnittstellen

- `SpotifyService`:
  - `authenticate(client_id, client_secret) -> Result<String, AuthError>`
  - `fetch_playlist(playlist_id) -> Result<Vec<SpotifyTrack>, ApiError>`
  - `fetch_track(track_id) -> Result<SpotifyTrack, ApiError>`
- `AudioPipeline`:
  - `match_and_download(track: &SpotifyTrack, cache_dir: &Path) -> Result<PathBuf, DownloadError>`
  - `convert_to_redbook_wav(input: &Path, output: &Path, normalize: bool) -> Result<(), AudioError>`
  - `generate_cuesheet(tracks: &[TrackAudio], output_cue: &Path) -> Result<(), CueError>`
- `DiscBurner` (Trait):
  - `detect_drives() -> Result<Vec<OpticalDrive>, BurnError>`
  - `get_media_status(drive_id: &str) -> Result<MediaStatus, BurnError>`
  - `burn_audio_cd(drive_id: &str, tracks_or_cue: &Path, speed: u32, progress_cb: Callback) -> Result<(), BurnError>`
  - `burn_data_cd(drive_id: &str, files_dir: &Path, speed: u32, progress_cb: Callback) -> Result<(), BurnError>`
  - `eject(drive_id: &str) -> Result<(), BurnError>`

---

## 4. Plattform-Treiber Matrix

| Plattform | Disc-Erkennung | Medienprüfung | Brenn-Engine |
| :--- | :--- | :--- | :--- |
| **macOS** | `drutil list -xml` | `drutil status -xml` | `drutil burn -audio` / DiscRecording |
| **Windows** | IMAPI2 `IDiscMaster2` | IMAPI2 `IDiscFormat2TrackAtOnce` | IMAPI2 COM / PowerShell Driver |
| **Linux** | `/proc/sys/dev/cdrom/info`, `wodim --devices` | `wodim -prcap` | `wodim -audio -dao -pad` / `xorriso` |
