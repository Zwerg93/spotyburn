# Task Backlog - SpotyBurn

## Phase 1: Core Systems (Completed)
- [x] TASK-001: Projektstruktur, Tauri v2 & Cargo Setup <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)
- [x] TASK-002: Spotify API Client & Pagination Engine <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)
- [x] TASK-003: Audio Sourcing & FFmpeg Red Book Transcoding Pipeline <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)
- [x] TASK-004: Cross-Platform Disc Burning Engine Abstraction <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)
- [x] TASK-005: Desktop UI Dashboard & Real-Time Log Bridge <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)
- [x] TASK-006: Multi-Plattform Build & CI/CD Packaging <!-- COMPLETED -->
  - **QA-Status:** Approved (PASS)

## Phase 2: v2.0 Extensions (Pending Approval)
- [x] TASK-007: Spotify User OAuth2 Loopback Flow & User Playlists Ingestion <!-- COMPLETED -->
  - **Kriterien:** Lokaler Loopback-Listener auf `http://127.0.0.1:8888/callback`; PKCE / Auth-Code Handshake; Refresh-Token Persistenz; Abruf von `GET /v1/me/playlists` mit Bildern, Titeln und Track-Counts; Logout-Funktion.
  - **QA-Status:** Approved (PASS)

- [x] TASK-008: In-App Spotify Search Engine <!-- COMPLETED -->
  - **Kriterien:** Anbindung von `GET /v1/search`; Suche nach Tracks, Playlists und Alben; DTOs und Tauri-Command `search_spotify`; 1-Klick-Übernahme gefundener Items in die Brennliste.
  - **QA-Status:** Approved (PASS)

- [x] TASK-009: Virtueller Test-Modus, dynamische Kapazitätsberechnung & Finder-Integration <!-- COMPLETED -->
  - **Kriterien:** Neuer Modus `BurnMode::ExportOnly` (kein physikalischer Brenner erforderlich); Herunterladen, Transkodieren zu 44.1kHz 16-Bit WAV mit 2352-Byte-Sektorausrichtung und CUE-Generierung; Tauri-Command `open_cache_folder` zum direkten Öffnen des Ordners im macOS Finder / Windows Explorer; Dynamische Kapazitätsberechnung im Backend (80 Min für Red Book, 700 MB / MP3-Größenschätzung für Data-CD, unlimitiert für Export).
  - **QA-Status:** Approved (PASS)

- [x] TASK-010: UI Modernisierung, Fenster-Entkopplung & reaktiver Kapazitätsbalken <!-- COMPLETED -->
  - **Kriterien:** Header mit ⚙️ Settings-Modal (Credentials, Cache-Dir) und 📜 Log-Terminal-Modal mit Status-Pille; linke Spalte mit Playlist-Karten (Cover, Titel, Track-Count); In-App-Suchleiste mit Dropdown/Vorschau; direkter Export-Button; **Reaktiver Kapazitätsbalken schaltet bei Moduswechsel sofort um**: Im Audio-CD-Modus zeigt er "X / 80 Min" (Ampelfarben bei 74m/80m), im MP3-Modus zeigt er "X MB / 700 MB" (mit ~120-160 Songs Kapazität); Warnungen passen sich dynamisch an.
  - **QA-Status:** Approved (PASS)