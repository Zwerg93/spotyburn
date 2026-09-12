# Spezifikation: SpotyBurn v2.0 (User Auth, In-App Search, Clean UI & Virtual Test Mode)

## 1. Übersicht & Scope-Grenzen

### 1.1 Ziele (In-Scope)
1. **Spotify User OAuth2 (PKCE / Authorization Code Flow):**
   - Lokaler Loopback-HTTP-Server auf `http://127.0.0.1:8888/callback`.
   - Scopes: `playlist-read-private`, `playlist-read-collaborative`, `user-library-read`.
   - Browserbasiertes Login, automatischer Token-Austausch und Refresh-Token-Persistenz in `~/.spotyburn/config.json`.
   - Abruf und Anzeige der persönlichen Playlists (`GET /v1/me/playlists`) mit Cover-Bildern, Name und Track-Anzahl.
2. **In-App Spotify Suche:**
   - Nativer Such-Endpunkt (`GET /v1/search?q=...&type=track,playlist,album`).
   - Sofortige Vorschau und 1-Klick-Import von Suchergebnissen in die Brennliste (ohne manuelles Kopieren von URLs).
   - Manuelle URL-Eingabe bleibt als Direkteingabe für externe Links erhalten.
3. **UI-Entkopplung (Clean Desktop Design):**
   - Sektion 1 (Spotify Setup & Authentifizierung) wandert in ein modales Einstellungsfenster (⚙️ Icon im Header).
   - Terminal-Logs wandern in ein aufrufbares Log-Fenster / Modal (📜 Icon im Header mit Live-Status-Indikator).
   - Hauptansicht:
     - Linke Spalte / Sidebar: Meine Playlists (Karten mit Cover, Titel, Track-Count).
     - Zentrale Ansicht: Suchleiste, Track-Tabelle mit dem 80-Minuten-Kapazitätsbalken.
4. **Virtueller Test-Modus ("Download & Export Only"):**
   - Modus "Virtueller Export / Test-Modus (Ohne Brenner)":
     - Lädt die Tracks via `yt-dlp` in den Cache/Export-Ordner.
     - Transkodiert zu 44.1 kHz 16-Bit PCM WAV mit exakter 2.352-Byte-Sektorausrichtung.
     - Generiert normgerechtes Red Book CUE-Sheet mit CD-Text.
     - Öffnet nach Abschluss direkt den Finder-/Explorer-Ordner mit den fertigen WAV-Dateien.

### 1.2 Nicht-Ziele (Out-of-Scope)
- Cloud-Streaming von Spotify-DRM geschützten Original-AAC-Streams.
- Dauerhaft laufender Hintergrund-Webserver nach erfolgtem OAuth2-Login.

---

## 2. Technische Architektur & Schnittstellen

### 2.1 OAuth2 Loopback Server
- Bei Start des Login-Prozesses bindet die App kurzzeitig einen TCP-Listener an `127.0.0.1:8888`.
- Nach Empfang von `/callback?code=...` sendet der Server eine elegante Erfolgs-HTML-Seite ("Anmeldung erfolgreich, du kannst dieses Fenster schließen") und schließt den Port sofort wieder.
- Der Code wird mit `client_id` und `client_secret` gegen ein Access- und Refresh-Token getauscht.

### 2.2 Datenmodelle (Erweiterung)

```rust
pub struct SpotifyPlaylistSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub track_count: u32,
    pub image_url: Option<String>,
}

pub struct SpotifySearchResult {
    pub tracks: Vec<SpotifyTrack>,
    pub playlists: Vec<SpotifyPlaylistSummary>,
}

pub struct UserProfile {
    pub display_name: String,
    pub id: String,
    pub image_url: Option<String>,
}
```

### 2.3 Erweiterte Tauri Commands
- `spotify_login()`: Startet Loopback-Server und öffnet Browser.
- `spotify_logout()`: Löscht gespeichertes Refresh-Token.
- `get_user_profile()`: Liefert Benutzerstatus und Profilbild.
- `get_user_playlists()`: Liefert alle Playlists des angemeldeten Benutzers.
- `search_spotify(query: String, search_type: String)`: Sucht nach Tracks, Alben oder Playlists.
- `open_cache_folder()`: Öffnet den Export-/Cache-Ordner nativ im macOS Finder bzw. Windows Explorer.
- `start_burn_job(...)`: Unterstützt nun `BurnMode::ExportOnly` (Virtueller Test ohne Brenner).
