# Preflight Risk Report: SpotyBurn v2.0 (User Auth, In-App Search & Dynamic Capacity)

## 1. Kritische Stolpersteine & Risiken

- **[UI & State / Moduswechsel]:** Das 80-Minuten-Limit springt beim Umschalten auf "Data / MP3 CD" nicht um.
  - *Ursache:* In `ui/js/app.js` war die Render-Funktion `updateCapacityBar()` fest an `totalMs` und das 80-Minuten-Zeitlimit gekoppelt, unabhängig vom ausgewählten `burn_mode`.
  - *Mögliche Auswirkung:* Verwirrung beim Benutzer; fälschliche Überlängenwarnung im MP3-Modus, obwohl auf eine 700-MB-Daten-CD hunderte Tracks passen.
  - *Empfohlene Gegenmaßnahme:* Reaktiver State-Handler:
    - **Audio CD (Red Book):** Einheit = Minuten (Limit: 80 Min / 4.800.000 ms).
    - **Data / MP3 CD:** Einheit = Megabyte (Limit: 700 MB / ~734.003.200 Bytes, dynamische Schätzung anhand Audio-Bitrate ca. 1,5–2 MB/Min oder tatsächlicher MP3-Dateigröße).
    - **Export Only (Testmodus):** Freier lokaler Festplattenspeicher, keine CD-Beschränkung.

- **[TASK-007 / Loopback Server]:** Port-Kollision auf `127.0.0.1:8888`.
  - *Mögliche Auswirkung:* Wenn Port 8888 von einem anderen Dienst blockiert ist, schlägt das Binden des Listeners fehl und der Login bricht ab.
  - *Empfohlene Gegenmaßnahme:* Sauberer Fehlerdialog in der UI mit präziser Meldung ("Port 8888 belegt"), automatischer Timeout nach 120 Sekunden Inaktivität.

- **[TASK-008 / In-App Suche]:** Token-Verfügbarkeit für `GET /v1/search`.
  - *Mögliche Auswirkung:* Suche schlägt fehl, wenn der Nutzer noch nicht persönlich eingeloggt ist.
  - *Empfohlene Gegenmaßnahme:* Dual-Token-Strategie: Wenn kein User-OAuth-Token vorliegt, nutzt `search_spotify` automatisch das vorhandene Client-Credentials-Token (`54469...`). Die Suche funktioniert somit **immer**!

---

## 2. Identifizierte Edge Cases

1. **Abbruch des OAuth2-Logins im Browser:**  
   Der Nutzer schließt den Tab oder bricht ab. -> Der Loopback-Listener wird nach einem Timeout von 120 Sekunden sauber beendet, ohne die App zu blockieren.
2. **Spotify Region-Locks (`is_playable == false`):**  
   Tracks in Playlists, die im Land des Nutzers gesperrt sind, werden in der Track-Tabelle mit einem Hinweis markiert und standardmäßig von der Brennliste abgewählt.
3. **Sehr lange Playlists im MP3-Modus (> 150 Tracks):**  
   Im MP3-Modus können 300+ Tracks geladen werden. Die UI-Tabelle verwendet Virtual Scrolling / optimiertes DOM-Rendering, um ruckelfrei zu bleiben.

---

## 3. Offene Klärungsfragen an den Entwickler

1. **MP3-Bitrate & Größenkalkulation:**  
   Für die Kapazitätsanzeige im MP3-CD-Modus vor dem eigentlichen Download: Sollen wir standardmäßig von einer 256 kbit/s MP3-Kodierung (~1,92 MB pro Minute Audio) ausgehen, um das verbleibende Kontingent der 700 MB präzise zu visualisieren? (Empfehlung: Ja).

---

## 4. Anpassungsempfehlungen für Backlog / Spec

- **Erweiterung TASK-009 & TASK-010:** Explizite Aufnahme der reaktiven Kapazitätsumschaltung (80 Min für Audio-CD vs. 700 MB für MP3-CD vs. Test-Export) in die Akzeptanzkriterien.
