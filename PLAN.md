# PLAN: Apexline – schnell veröffentlichen

Stand: 08.10.2026. Ziel: **v0.1.0 so bald wie möglich öffentlich**, mit dem, was schon läuft. Alles
Weitere kommt danach, gesteuert von eigener Nutzung und Rückmeldungen – nicht vorab auf Vorrat.

---

## 1. Leitlinien

- **Erst veröffentlichen, dann ausbauen.** v0.1 enthält nur Fertiges. Keine Einstellungen, Menüpunkte
  oder Issue-Vorlagen für Funktionen, die es noch nicht gibt.
- **Nicht overengineeren:** kein eigener Server, keine Online-Datenbank, keine Installer. Ein Programm
  zum Herunterladen und Starten; Autostart erst einmal per Anleitung.
- Bleibt wie entschieden: Name **Apexline** (kein „GT7/Gran Turismo“ im Namen, nur beschreibend),
  Lizenz MIT OR Apache-2.0, Code-Kommentare Englisch, Oberfläche/Coach Deutsch + Englisch (Einheiten
  folgen der Sprache), Coach in Rust im Server, schlanke Tests + CI, kein PIN, keine Töne.
- Zielplattformen der Programme: Linux x86_64, Raspberry Pi (Linux aarch64), Windows x86_64,
  macOS (Apple Silicon + Intel).

---

## 2. Was v0.1 schon kann

- **Server** (`apexline`): findet die PS5 selbst (Broadcast, gemerkte Adresse) oder feste IP;
  Live-Dashboard per WebSocket; Aufzeichnung jeder Runde (CSV, 60/s) mit Session-Erkennung.
- **Dashboard** (iPhone-optimiert, 4 Seiten): Fahren, alle Daten, Coach, Messungen; Einstellungen inkl.
  Server-Abschnitt (PS5-IP, Port, Sprache, Einheiten); Hinweis bei fehlenden Daten; OLED-Schutz.
- **Coach** (regelbasiert): Kurvenvergleich mit der Bestrunde, Trail-Braking-Bewertung, Fokus,
  Session-Zusammenfassung; `apexline analyze` auf der Konsole.
- **Messungen**: 0–100/0–60 mph, Zwischenspurts, 1/4 Meile, Bremswege, beste Schaltpunkte und
  Leistungsband (ohne Gewichtsangabe), Bestwerte je Auto; `apexline perf` für alte Aufnahmen.
- **Konfiguration** `config.toml` im OS-Ordner, Daten im OS-Datenordner, `apexline config` zeigt beides.
- Deutsch/Englisch, metrisch/imperial; 13 Tests, clippy ohne Warnungen.
- Repo öffentlich: github.com/Pflegusch/apexline (Lizenzen, CONTRIBUTING, Issue-Vorlagen vorhanden).
- Quellen/Lizenzen geprüft: Fahrzeugliste `data/*.csv` aus ddm999/gt7info (MIT-0), Paketformat nach
  Nenkai/PDTools (MIT, nur das Format nachgebaut) – Nennung in der README genügt.

**Bekannte Grenzen (kommen so in die README):** keine Streckenerkennung (Kurven heißen K1/T1 …, pro
Session neu nummeriert); Live-Delta nur gegen die Bestrunde der laufenden Session; Rohdaten wachsen
(~4 MB pro Runde, kein automatisches Aufräumen); Coach-Grenzwerte an wenigen Autos abgestimmt;
Schaltpunkte nur so gut wie die Vollgas-Läufe durch mehrere Gänge.

---

## 3. Weg zu v0.1.0 (≈ 2–3 Arbeitstage)

### R1 – Aufräumen (½ T) – ✅ 08.10.
- Konfiguration ohne Funktion entfernen: `retention_days`, `track_db_url`, `online_tracks` (aus
  `config.rs`, `/api/settings`, README); kommen zurück, wenn es die Funktion gibt
- Issue-Vorlage „Strecke/Kurvennamen“ entfernen (gibt es erst mit Streckenerkennung)
- `rustfmt.toml` (`max_width = 140`, `use_small_heuristics = "Max"`) + einmal `cargo fmt` (≈ 34 Stellen)
- `CHANGELOG.md` mit v0.1.0; Version 0.1.0 bleibt
- **Abnahme:** keine Einstellung, Option oder Vorlage ohne Funktion; `cargo fmt --check` sauber

### R2 – CI (½ T) – ✅ 08.10. (grün auf Linux, Windows, macOS)
- GitHub Actions bei jedem Push/PR: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test` (Linux) + Build-Prüfung für Windows und macOS (findet Plattformfehler früh)
- **Abnahme:** CI grün auf `main`

### R3 – Programme (1 T) – ✅ 08.10. (Linux statisch per musl; Test-Build aller fünf Archive grün)
- Workflow bei Tag `v*`: Release-Builds für `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`
  (ARM-Runner, für den Raspberry Pi), `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`,
  `x86_64-apple-darwin`; je ein Archiv mit Programm, README, Lizenzen; SHA-256-Prüfsummen;
  automatisch ein GitHub-Release (Entwurf) mit den Dateien
- Dashboard ist bereits eingebettet → eine Datei pro Plattform
- Änderungen an `release.yml` bauen die Archive zum Test (ohne Release); der Entwurf entsteht erst beim Tag
- **Abnahme:** alle fünf Archive werden gebaut

### R4 – README & Screenshots (½–1 T) – ✅ 08.10.
- `README.md` nur auf Englisch, kurz, aufs Wesentliche (Wunsch: keine deutsche Fassung): Screenshots,
  Funktionen, Schnellstart je Plattform, Fehlersuche, Verweise (Demo, analyze, systemd, Changelog), Lizenz
- 4 Screenshots vom iPhone (Web-App, Hochformat, Englisch) in `docs/img/`: Fahren, Coach, Messungen,
  Schaltpunkte/Leistungsband – zugeschnitten (ohne Statusleiste), 600 px breit, in der README nebeneinander
- **Abnahme:** jemand ohne Vorwissen kommt nur mit der README zum laufenden Dashboard

### R5 – Prüfen & veröffentlichen (¼ T)
- ✅ **Frische Installation gegen die echte PS5** (leerer Daten-/Konfigordner, keine Parameter):
  per Broadcast in wenigen Sekunden gefunden und gemerkt (08.10.)
- Linux-Archiv aus dem Release herunterladen und starten; Windows/macOS/Pi wenn ein Gerät da ist,
  sonst in den Release-Notes als „ungetestet“ kennzeichnen
- ✅ Tag `v0.1.0` gesetzt (08.10.), Release-Entwurf mit 5 Archiven, Prüfsummen und Text aus dem CHANGELOG
  automatisch erstellt; offen: Entwurf auf GitHub veröffentlichen
- Eigenes System aufräumen (5 min): alte Unit `gt7-telemetry.service`, `~/.local/share/gt7-legacy`,
  Symlinks `recordings`/`coach`/`perf` im Projektordner
- **Abnahme:** Release mit Programmen online, eigenes System läuft auf der Release-Version

### Entschieden
- E-Mail-Adresse in den Commits bleibt wie sie ist (kein Umschreiben der Historie).
- Tag `v0.1.0` erst, wenn ich ihn freigebe.

---

## 4. Danach – Backlog (nach Rückmeldungen priorisieren)

Grobe Reihenfolge nach Nutzen/Aufwand; nichts davon ist zugesagt.

1. **Speicher** (½–1 T): Runden gzip-komprimiert schreiben (~6× kleiner, `flate2`), alte Rohdaten nach
   N Tagen löschen (Analysen, Messungen und Bestrunden bleiben) – sobald der Platz stört.
2. **Bestzeit über Sessions** (1 T): beste Runde je Auto + Strecke merken und als Live-Delta-Referenz
   laden; braucht eine einfache Streckenzuordnung (gleiche Rundenlänge ±1 % + Startpunkt).
3. **Autostart-Befehl** (1 T): `apexline service install` (systemd-User-Dienst, launchd-Agent,
   Windows-Autostart) statt Anleitung.
4. **Session-Browser** (2–3 T): Liste der Sessions, Rundentabelle, zwei Runden über die Distanz
   vergleichen (Tempo, Gas, Bremse, Δ-Zeit); Messläufe auswählen/löschen.
5. **Strecken & Kurvennamen** (3–5 T): Umriss einer sauberen Runde als Streckendatei speichern,
   beim nächsten Mal geometrisch wiedererkennen (mittlerer Abstand < 15 m); Kurvennamen aus Dateien im
   Repo-Ordner `tracks/` (per Pull Request beigesteuert), zuerst Spa, Monza, Nordschleife, Suzuka.
6. **Kleinere Ideen:** Schaltblitz am optimalen Schaltpunkt, Schaltpunkte im Coach, Sektorzeiten,
   Replay einer Session, CSV-Export einer Runde.
7. **Nur bei Nachfrage:** MoTeC-`.ld`-Export, Rennstrategie/Sprit bis ins Ziel, Tablet-Ansicht,
   MQTT, KI-Analyse mit eigenem API-Schlüssel, native iOS-App.

---

## 5. Erledigt (Kurzfassung, Details in den Commits)

| Was | Wann |
|---|---|
| Coach von Python nach Rust portiert, Gleichstand auf 3 Sessions geprüft, läuft im Server | 06.10. |
| Messungen (Beschleunigung, Bremswege, Bestwerte) inkl. `perf`-Befehl, Dashboard-Seite 4 | 08.10. |
| Umbenennung in Apexline, Module nach Zielstruktur, Lizenzen, CONTRIBUTING, englische Kommentare | 08.10. |
| `config.toml` + OS-Datenordner, PS5-Suche per Broadcast, Server-Einstellungen im Dashboard | 08.10. |
| Eigenes System auf den Dienst `apexline` umgestellt (Daten in `~/.local/share/apexline`) | 08.10. |
| Deutsch/Englisch + metrisch/imperial überall; mph-Marken; Schaltpunkte statt Gewichtseingabe | 08.10. |
| Repo auf GitHub (öffentlich) | 08.10. |

### Technische Notizen
- **Module:** `telemetry/` (Empfang, Entschlüsselung, PS5-Suche), `session/` (Tracker, Recorder,
  Streckenkarte), `coach/`, `perf/` (Erkennung, Schaltpunkte, Speicher), `web/` (Server, API),
  `config.rs`, `i18n.rs` (`tr!("de", "en")` + Einheiten-Helfer), Dashboard `web/index.html` +
  `web/i18n.js` (deutscher Text = Schlüssel, `EN`-Tabelle).
- **Daten:** `<Datenordner>/recordings/<Datum>_<Auto>/` (CSV je Runde, `session.json`, `analysis/`),
  `coach/feed.json`, `perf/<Fahrzeug-ID>/*.json`, `state.json` (gefundene PS5).
- **Eigenes System:** Dienst `apexline` (systemd-User-Dienst aus `~/.cargo/bin/apexline`); neue Version
  mit `cargo install --path . --locked` + `systemctl --user restart apexline`, nur wenn nicht gefahren
  wird. Paralleltest ohne Konflikt: `apexline demo --port 8099 --config <tmp> --data-dir <tmp>`.
- **Stolperstellen:** Trail-Braking-Themen haben deutsche IDs (in `analysis/state.json` gespeichert);
  Kurvenrichtung in `corners.json` ist „links/rechts“; Schaltpunkte brauchen Leistungsdaten über den
  Drehzahlbereich nach dem Hochschalten, sonst „spät“.
