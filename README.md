# SpotyBurn 💿🔥

> **Cross-Platform Spotify Audio CD Desktop App**  
> Konvertiere Spotify-Playlists und Alben direkt in standardkonforme **Red Book Audio-CDs (CD-DA)** oder MP3-Daten-CDs für jedes Autoradio und jeden Standalone-CD-Player.

[![CI](https://github.com/Zwerg93/spotyburn/actions/workflows/ci.yml/badge.svg)](https://github.com/Zwerg93/spotyburn/actions/workflows/ci.yml)
[![Release](https://github.com/Zwerg93/spotyburn/actions/workflows/release.yml/badge.svg)](https://github.com/Zwerg93/spotyburn/actions/workflows/release.yml)
[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](#systemvoraussetzungen)
[![Runtime](https://img.shields.io/badge/Runtime-Rust%20%2B%20Tauri%20v2-orange.svg)](https://tauri.app/)

---

## 📋 Inhaltsverzeichnis

- [Überblick](#-überblick)
- [Hauptfunktionen](#-hauptfunktionen)
- [Systemvoraussetzungen](#-systemvoraussetzungen)
- [Schnellstart & Schritt-für-Schritt Anleitung](#-schnellstart--schritt-für-schritt-anleitung)
  - [1. Spotify API Credentials (.env oder In-App)](#1-spotify-api-credentials-env-oder-in-app)
  - [2. Anmeldung mit Spotify & Eigene Playlists](#2-anmeldung-mit-spotify--eigene-playlists)
  - [3. In-App Suche oder URL-Direkteingabe](#3-in-app-suche-oder-url-direkteingabe)
  - [4. Reaktive Kapazitätskontrolle (80 Min vs. 700 MB)](#4-reaktive-kapazitätskontrolle-80-min-vs-700-mb)
  - [5. Brennen oder Virtueller Test-Export](#5-brennen-oder-virtueller-test-export)
- [Entwicklungsworkflow (`make`)](#-entwicklungsworkflow-make)
- [Multi-Plattform Build & Packaging](#-multi-plattform-build--packaging)
  - [macOS (.dmg / .app)](#macos-dmg--app)
  - [Windows (.msi / .exe)](#windows-msi--exe)
  - [Linux (.deb / .AppImage)](#linux-deb--appimage)
- [Architekturüberblick](#-architekturüberblick)
- [Fehlerbehebung (Troubleshooting)](#-fehlerbehebung-troubleshooting)

---

## 🌟 Überblick

Moderne Streaming-Dienste bieten unendliche Musikvielfalt, aber viele klassische Auto-HiFi-Anlagen, Youngtimer und Standalone-CD-Player beherrschen ausschließlich physische **Red Book Audio-CDs (CD-DA)**. 

**SpotyBurn** schließt diese Lücke: Es verbindet sich über die offizielle Spotify Web API mit deiner Bibliothek oder öffentlichen Playlists, lädt hochauflösende Audiospuren vollautomatisch herunter, transkodiert sie via FFmpeg in bitgenaue 44.1 kHz / 16-Bit Stereo PCM WAV-Dateien mit exakter 2.352-Byte-Sektorausrichtung und brennt sie nativ auf CD-R/CD-RW – ganz ohne externe Brennprogramme.

---

## ✨ Hauptfunktionen

- **User-OAuth2 Login (PKCE):** 1-Klick-Anmeldung über deinen Standardbrowser (`http://127.0.0.1:8888/callback`). Permanenter Login über verschlüsselte lokale Refresh-Tokens.
- **Eigene Playlists auf einen Klick:** Die Sidebar lädt automatisch alle deine Playlists mit Cover-Art, Titel und Song-Anzahl.
- **In-App Spotify Suche:** Nativ in der App nach Tracks, Alben und Playlists suchen und mit einem Klick zur Brennliste hinzufügen.
- **Red Book CD-DA Standard (IEC 60908):**
  - Bitgenaue 44.1 kHz, 16-Bit Stereo PCM WAV-Transkodierung.
  - **Exakte Sektorenausrichtung an 2.352-Byte-Blöcken (1/75 s):** Verhindert Knackser an Trackgrenzen und Lesefehler in Autoradios.
  - **EBU R128 Lautheitsnormalisierung:** Einheitliche Lautstärke über alle Titel hinweg (-16 LUFS Target, -1.0 dBTP True Peak).
  - Normgerechte CUE-Sheets mit CD-Text (Titel, Interpret, ISRC, 2s Pregap auf Track 1).
- **Reaktiver Kapazitätsbalken:**
  - *Audio CD (Red Book):* Schaltet auf Zeitmessung (max. **80 Minuten** / 4.800.000 ms) mit Ampelfarben (<74m Grün, 74–80m Gelb, >80m Rot mit Warnung).
  - *Data / MP3 CD:* Schaltet sofort auf Dateigröße (**700 MB**) um; fasst locker 120–160 Songs!
  - *Virtueller Test (Export):* Unbegrenzte Kapazität für lokalen Ordner-Export.
- **Virtueller Test-Modus:** Teste die komplette Transkodierungs- und CUE-Pipeline auch ohne angeschlossenes CD-Laufwerk; öffnet den fertigen Ordner direkt im Finder/Explorer.
- **Native Optical Burning Engines (Selbsttragend):**
  - **macOS:** Bordeigenes Apple `drutil` & DiscRecording Subsystem.
  - **Windows:** Windows Image Mastering API v2 (IMAPI2) COM-Integration.
  - **Linux:** POSIX-Treiber via `wodim` / `xorriso`.
- **Entkoppelte Desktop-UI:** Einstellungen (⚙️) und Terminal-Logs (📜) sind als elegante Modals aus der Hauptansicht entkoppelt.

---

## 💻 Systemvoraussetzungen

### Unterstützte Betriebssysteme:
- **macOS:** macOS 10.15 (Catalina) oder neuer (Apple Silicon M1/M2/M3/M4 & Intel x86_64).
- **Windows:** Windows 10 (Build 1809+) oder Windows 11 (64-Bit x64 oder ARM64).
- **Linux:** Ubuntu 20.04+, Debian 11+, Fedora 36+, Arch Linux mit `libwebkit2gtk-4.1`.

### Hardware:
- Internes oder externes optisches CD/DVD/BD-Brenner-Laufwerk (USB / SATA). *(Nicht erforderlich im virtuellen Test-Export-Modus).*
- Leere CD-R Rohlinge (empfohlen für Autoradios) oder CD-RW (700 MB / 80 Minuten).

---

## 🚀 Schnellstart & Schritt-für-Schritt Anleitung

### 1. Spotify API Credentials (.env oder In-App)
1. Öffne das [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Melde dich an, erstelle eine App und trage unter **Redirect URIs** ein:
   ```
   http://127.0.0.1:8888/callback
   ```
3. Kopiere **Client ID** und **Client Secret**.
4. **Zwei Möglichkeiten zur Konfiguration:**
   - **Variante A (Empfohlen für Entwickler):** Erstelle eine `.env`-Datei im Projektverzeichnis:
     ```bash
     cp .env.example .env
     # Trage deine Keys in .env ein
     ```
   - **Variante B (Über die GUI):** Klicke in SpotyBurn oben rechts auf das ⚙️ Zahnrad und trage die Keys ein.

### 2. Anmeldung mit Spotify & Eigene Playlists
1. Klicke im Header auf **Mit Spotify anmelden**.
2. Dein Standardbrowser öffnet sich; bestätige den Lesezugriff auf deine Playlists.
3. Deine Playlists erscheinen sofort in der linken Sidebar. Klicke auf eine Playlist, um alle Songs zu laden.

### 3. In-App Suche oder URL-Direkteingabe
- Nutze die Suchleiste im Hauptfenster, um nach beliebigem Titel, Album oder Künstler zu suchen.
- Alternativ kannst du auch jede öffentliche Spotify-URL direkt einfügen.

### 4. Reaktive Kapazitätskontrolle (80 Min vs. 700 MB)
- Der Kapazitätsbalken passt sich sofort an deinen gewählten Modus an:
  - Bei **Audio CD** siehst du die Minuten (`X / 80 Min`).
  - Bei **Data / MP3 CD** wechselt die Anzeige auf Megabyte (`X MB / 700 MB`).

### 5. Brennen oder Virtueller Test-Export
- **Mit CD-Brenner:** Wähle dein Laufwerk und die Geschwindigkeit (empfohlen: **4x** oder **8x** für maximale Laser-Kompatibilität). Klicke auf **Download & Brennen**.
- **Ohne CD-Brenner (Test-Modus):** Wähle den Tab **🧪 Test-Export**. Klicke auf **Download & CUE exportieren**. SpotyBurn lädt alles herunter, transkodiert zu Red Book WAV und öffnet automatisch den Zielordner im Finder/Explorer.

---

## 🛠 Entwicklungsworkflow (`make`)

```bash
# Projekt im Entwicklungsmodus kompilieren
make build

# Installierbare macOS .app und .dmg bauen
make dmg

# Alle 85+ Unit- und Integrationstests ausführen
make test

# Code-Formatting und Clippy-Linter prüfen
make lint

# Code automatisch formatieren
make fmt

# Build-Artefakte bereinigen
make clean
```

---

## 📦 Multi-Plattform Build & Packaging

### macOS (.dmg / .app)
```bash
make dmg
# Erzeugt: target/bundle/osx/SpotyBurn.dmg und SpotyBurn.app
```

### Windows (.msi / .exe)
```bash
cargo tauri build
# Erzeugt: src-tauri/target/release/bundle/msi/*.msi und spotyburn.exe
```

### Linux (.deb / .AppImage)
```bash
cargo tauri build
# Erzeugt: src-tauri/target/release/bundle/deb/*.deb und *.AppImage
```

---

## 🏗 Architekturüberblick

Ausführliche Spezifikationen, mathematische Sektorberechnungen und OS-Treiber-Details findest du in [`ARCHITECTURE.md`](ARCHITECTURE.md).

```
┌────────────────────────────────────────────────────────┐
│            SpotyBurn Dashboard (HTML5/CSS3/ES6)        │
│    [Sidebar: Playlists]  [Search & Tracks]  [Modals]   │
└───────────────────────────┬────────────────────────────┘
                            │ Tauri IPC (Commands & Events)
┌───────────────────────────▼────────────────────────────┐
│                    Rust Core Engine                    │
│ ┌───────────────────┐ ┌───────────────────┐ ┌────────┐ │
│ │  Spotify Client   │ │   Audio Pipeline  │ │ Config │ │
│ │(OAuth2 PKCE/Search│ │ (FFmpeg / yt-dlp) │ │ (.env) │ │
│ └───────────────────┘ └─────────┬─────────┘ └────────┘ │
│                                 │ 2352-Byte Sector WAV │
│ ┌───────────────────────────────▼────────────────────┐ │
│ │              DiscBurner Trait Engine               │ │
│ │  ┌──────────────┐  ┌──────────────┐  ┌───────────┐ │ │
│ │  │ macOS: drutil│  │ Windows:IMAPI│  │Linux:wodim│ │ │
│ │  └──────────────┘  └──────────────┘  └───────────┘ │ │
│ └────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────┘
```

---

## ❓ Fehlerbehebung (Troubleshooting)

### 1. Kein optisches Laufwerk angeschlossen?
- Nutze einfach den Modus **"🧪 Test-Export"**. SpotyBurn benötigt in diesem Modus kein CD-Laufwerk und exportiert alle 44.1 kHz WAV-Dateien inklusive CUE-Sheet direkt in deinen lokalen Ordner.

### 2. Pufferüberlauf / Schreibfehler beim Brennen
- Reduziere die Brenngeschwindigkeit auf **4x** oder **8x**. Hohe Geschwindigkeiten erzeugen auf älteren CD-Laufwerken Lesefehler.
- Verwende hochwertige Markenrohlinge (z. B. Verbatim AZO CD-R).

### 3. Port 8888 für Spotify-Login belegt
- Falls eine andere Anwendung Port 8888 nutzt, beende diese kurzzeitig vor dem Klick auf "Mit Spotify anmelden". Nach dem Login schließt SpotyBurn den Port sofort wieder.

---

## 📄 Lizenz

Dieses Projekt steht unter der [MIT License](LICENSE).
