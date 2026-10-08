// Texts and units of the dashboard.
//
// German is the source language: the German text is the key, EN holds the English text.
// t('Runde {n}', { n: 3 }) → "Runde 3" / "Lap 3". In index.html, elements with `data-t` are
// translated by translatePage(), `data-t-ph` translates a placeholder and `data-u` shows a unit.
// Language and units come from the server (config.toml, injected into <script id="server-settings">):
// "auto" = browser language, and units follow the language (German metric, English imperial).
// The server sends everything metric; U converts for display.
(() => {
  const EN = {
    // Page 1
    'VL': 'FL', 'VR': 'FR', 'HL': 'RL', 'HR': 'RR',
    'Strecke': 'Track',
    'Karte entsteht beim Fahren': 'The map is drawn as you drive',
    'Balance': 'Balance',
    'unter': 'under',
    'über': 'over',
    'Runde': 'Lap',
    'Δ Live': 'Δ live',
    'Letzte': 'Last',
    'Beste': 'Best',
    'Sprit': 'Fuel',
    'Ø/Runde': 'Avg/lap',
    'Reicht für': 'Range',
    'Wasser': 'Water',
    'Öl': 'Oil',
    'Ladedruck': 'Boost',
    'U/min': 'rpm',
    'BLOCK': 'LOCK',
    'SCHLUPF': 'SPIN',
    'Neutral': 'Neutral',
    'Untersteuern': 'Understeer',
    'Übersteuern': 'Oversteer',
    'Rd.': 'laps',
    'Pause': 'Paused',

    // Page 2
    'G-Kräfte': 'G forces',
    'Quer': 'Lateral',
    'Längs': 'Longitudinal',
    'Vertikal': 'Vertical',
    'Max. quer / Bremsen': 'Max. lateral / braking',
    'Lenkung & Balance': 'Steering & balance',
    'Lenkwinkel': 'Steering angle',
    'Gierrate': 'Yaw rate',
    'Schwimmwinkel': 'Body slip angle',
    'Schräglauf vorne': 'Slip angle front',
    'Schräglauf hinten': 'Slip angle rear',
    'Balance (+ unter / − über)': 'Balance (+ under / − over)',
    'Quergeschwindigkeit (+ links)': 'Lateral speed (+ left)',
    'Lenkübersetzung (gelernt)': 'Steering ratio (learned)',
    'Räder': 'Wheels',
    'Temp.': 'Temp.',
    'Schlupf %': 'Slip %',
    'Federweg': 'Suspension',
    'Pedale & Antrieb': 'Pedals & drivetrain',
    'Gas': 'Throttle',
    'Bremse': 'Brake',
    'Kupplung': 'Clutch',
    'Kraftschluss': 'Engagement',
    'Drehzahl': 'Engine speed',
    'Drehzahl Getriebe': 'Gearbox speed',
    'Gang / Empfehlung': 'Gear / suggested',
    'Energierückgewinnung': 'Energy recovery',
    'kein Turbo': 'no turbo',
    'Motor & Sprit': 'Engine & fuel',
    'Wassertemperatur': 'Water temperature',
    'Öltemperatur': 'Oil temperature',
    'Öldruck': 'Oil pressure',
    'Verbrauch Ø/Runde': 'Fuel per lap (avg)',
    '{n} Runden': '{n} laps',
    'Fahrzeug & Getriebe': 'Car & gearbox',
    'Fahrzeug': 'Car',
    'Fahrzeug-ID': 'Car ID',
    'Fahrzeug {code}': 'Car {code}',
    'unbekannt': 'unknown',
    'Max. Tempo (berechnet)': 'Top speed (calculated)',
    'Schaltblitz': 'Shift light',
    'Übersetzungen': 'Gear ratios',
    'Lage & Position': 'Attitude & position',
    'Kurs': 'Heading',
    'Nicken / Rollen': 'Pitch / roll',
    'Fahrzeughöhe': 'Ride height',
    'Tempo': 'Speed',
    'Runden': 'Laps',
    'Runde {n}': 'Lap {n}',
    'Laufend': 'Current',
    'Δ Live zur Bestrunde': 'Live Δ to best lap',
    'Letzte − Beste': 'Last − best',
    'Tageszeit (Spiel)': 'Time of day (game)',
    'Paketformat': 'Packet format',
    'Aufzeichnung': 'Recording',
    'erweitert (344 Bytes)': 'extended (344 bytes)',
    'einfach (296 Bytes)': 'basic (296 bytes)',
    'läuft': 'on',
    'aus': 'off',
    'Auf Strecke': 'On track',
    'Lädt': 'Loading',
    'Gang drin': 'In gear',
    'Begrenzer': 'Limiter',
    'Handbremse': 'Handbrake',
    'Licht': 'Lights',
    'Fernlicht': 'High beam',

    // Page 3
    'Fokus': 'Focus',
    'Stand {time}': 'As of {time}',
    'Noch kein Feedback.': 'No feedback yet.',

    // Page 4
    'Messung': 'Measurements',
    'Messung läuft': 'Measuring',
    'Bremse oder Gas weg beendet': 'braking or lifting ends it',
    'Dieser Lauf': 'This run',
    'Bremsung': 'Braking',
    'bis zum Stillstand bremsen': 'brake to a standstill',
    'Letzter Lauf': 'Last run',
    'letzter Lauf': 'last run',
    'Bereit': 'Ready',
    'Gas geben startet die Messung': 'Throttle starts the measurement',
    'Keine Messung': 'Idle',
    'Zum Messen anhalten': 'Stop to measure',
    'Anhalten und Gas geben misst die Beschleunigung (zählt ab {v}). Vollbremsung ab {v} bis zum Stillstand misst den Bremsweg.':
      'Stop, then accelerate to measure acceleration (counts from {v}). Full braking from {v} to a standstill measures the braking distance.',
    'Meile': 'mile',
    'Steigung': 'uphill',
    'Gefälle': 'downhill',
    '★ Best': '★ Best',
    'Tempo über Zeit': 'Speed over time',
    'Noch keine Beschleunigung gemessen.': 'No acceleration measured yet.',
    'Bestlauf': 'Best run',
    'Letzter Lauf = Bestlauf': 'Last run = best run',
    'Live': 'Live',
    'Tippen zeigt die Werte': 'Tap to show the values',
    'Schaltpunkte': 'Shift points',
    'Noch keine Auswertung: dafür mit Vollgas durch mehrere Gänge beschleunigen.': 'Nothing to show yet: accelerate at full throttle through several gears.',
    'Gang': 'Gear',
    'optimal': 'best',
    'gefahren': 'driven',
    'spät': 'late',
    'bis': 'up to',
    'Leistungsband': 'Power band',
    '% vom Maximum': '% of maximum',
    'Leistung': 'Power',
    'Drehmoment': 'Torque',
    'Max. Leistung bei {rpm}': 'Max. power at {rpm}',
    'Max. Drehmoment bei {rpm}': 'Max. torque at {rpm}',
    'Leistungsgewicht am Rad: {v}': 'Power-to-weight at the wheels: {v}',
    'Lauf vom {date}': 'Run of {date}',
    'Berechnet aus der Beschleunigung bei Vollgas, ohne Gewichtsangabe. Optimal schalten heißt: im Gang bleiben, solange er bei gleichem Tempo mehr Leistung auf die Straße bringt als der nächste Gang nach dem Schalten. „spät“ = bis zum Begrenzer ausdrehen.':
      'Calculated from the acceleration at full throttle, no weight needed. Shifting at the best point means staying in a gear as long as it puts more power on the road at the same speed than the next gear would after the shift. “late” = rev up to the limiter.',
    'Bestwerte': 'Best values',
    'Noch keine Bestwerte für dieses Auto.': 'No best values for this car yet.',
    'Werte mit mehr als {g} % Steigung oder Gefälle zählen nicht als Bestwert.': 'Values measured on more than {g} % slope do not count as best values.',
    'Letzte Läufe': 'Recent runs',
    'Bremsung aus {v}': 'Braking from {v}',
    'gesamt': 'total',
    'Noch keine Läufe.': 'No runs yet.',

    // Start, settings, connection
    'Telemetrie-Dashboard & Coach für Gran Turismo 7': 'Telemetry dashboard & coach for Gran Turismo 7',
    'Tippen startet das Dashboard und hält den Bildschirm wach. Wischen wechselt zwischen Fahr-Ansicht, allen Daten, dem Coach-Feedback und den Messungen (0–100, Bremswege, Schaltpunkte), langes Drücken öffnet die Einstellungen. Tipp: Über „Teilen → Zum Home-Bildschirm“ läuft es im Vollbild.':
      'Tap to start the dashboard and keep the screen awake. Swipe to switch between driving view, all data, coach feedback and measurements (0–60, braking distances, shift points); a long press opens the settings. Tip: “Share → Add to Home Screen” runs it full screen.',
    'Starten': 'Start',
    'Einstellungen': 'Settings',
    'Reifenfenster in {u}. Unter „kalt“ blau, im Fenster grün, darüber gelb bis rot. Die Obergrenzen der Racing-Presets sind Community-Messungen, ab denen der Verschleiß überproportional steigt. Die Untergrenze ist ein Schätzwert.':
      'Tyre window in {u}. Blue below “cold”, green inside the window, yellow to red above. The upper limits of the racing presets are community measurements above which wear rises disproportionately. The lower limit is an estimate.',
    'Reifen': 'Tyres',
    'Eigene Werte': 'Custom',
    'Kalt unter': 'Cold below',
    'Optimal bis': 'Optimal up to',
    'Überhitzt ab': 'Overheated from',
    'Glättung (Sekunden)': 'Smoothing (seconds)',
    'Karte spiegeln': 'Mirror map',
    'Helligkeit': 'Brightness',
    'Burn-in-Schutz (Pixel-Shift)': 'Burn-in protection (pixel shift)',
    'Sprache': 'Language',
    'Einheiten': 'Units',
    'Automatisch': 'Automatic',
    'Metrisch (km/h, °C)': 'Metric (km/h, °C)',
    'Imperial (mph, °F)': 'Imperial (mph, °F)',
    'automatisch': 'automatic',
    'PS5-IP': 'PS5 IP',
    'Dashboard-Port': 'Dashboard port',
    'Fertig': 'Done',
    'Standardwerte': 'Defaults',
    'Server nicht erreichbar': 'Server not reachable',
    'Port {p} gilt nach einem Neustart': 'port {p} applies after a restart',
    'Daten': 'Data',
    'Konfiguration': 'Configuration',
    'PS5-IP: z. B. 192.168.0.33, leer = automatisch': 'PS5 IP: e.g. 192.168.0.33, empty = automatic',
    'Antwort ungültig': 'Invalid response',
    'Nicht gespeichert: {e}': 'Not saved: {e}',
    'Wake Lock aktiv': 'Wake lock active',
    'Wachhalten per Video aktiv': 'Kept awake by a video',
    'Wachhalten nicht möglich – Auto-Sperre in iOS manuell abschalten': 'Cannot keep the screen awake – turn off Auto-Lock in iOS manually',
    'Keine Daten von GT7': 'No data from GT7',
    'PS5 an und GT7 gestartet? Rechner und PS5 im selben Netz? Firewall: UDP 33739 und 33740 frei? Die PS5-IP lässt sich in den Einstellungen fest eintragen (lange drücken).':
      'PS5 on and GT7 running? Computer and PS5 on the same network? Firewall: UDP 33739 and 33740 open? The PS5 IP can be set in the settings (long press).',
    'Verbunden mit PS5 {ip}': 'Connected to PS5 {ip}',
    'Verbunden': 'Connected',
    'Keine Verbindung zum Apexline-Server': 'No connection to the Apexline server',
    'Sende an {ip} – keine Antwort': 'Sending to {ip} – no answer',
    'Suche PS5 (zuletzt {ip}) und per Broadcast …': 'Searching for the PS5 (last {ip}) and by broadcast …',
    'Suche PS5 per Broadcast …': 'Searching for the PS5 by broadcast …',
  };

  let cfg = {};
  try { cfg = JSON.parse(document.getElementById('server-settings').textContent); } catch {}
  const browserDe = (navigator.language || '').toLowerCase().startsWith('de');
  const lang = cfg.language === 'de' || cfg.language === 'en' ? cfg.language : (browserDe ? 'de' : 'en');
  const imperial = cfg.units === 'imperial' || (cfg.units !== 'metric' && lang === 'en');

  function t(text, vars) {
    let s = lang === 'en' ? (EN[text] ?? text) : text;
    if (vars) s = s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? vars[k] : m));
    return s;
  }

  const U = imperial
    ? {
        speed: 'mph', spd: kmh => kmh / 1.609344,
        temp: '°F', tmp: c => c * 9 / 5 + 32, tmpInv: f => (f - 32) * 5 / 9,
        dist: 'ft', dst: m => m * 3.28084,
        fuel: 'gal', fl: l => l * 0.264172,
        press: 'psi', prs: bar => bar * 14.5038,
        lenUnit: 'in', travel: mm => mm / 25.4, radius: cm => cm / 2.54,
      }
    : {
        speed: 'km/h', spd: v => v,
        temp: '°C', tmp: c => c, tmpInv: c => c,
        dist: 'm', dst: m => m,
        fuel: 'L', fl: l => l,
        press: 'bar', prs: b => b,
        lenUnit: 'mm', travel: mm => mm, radius: cm => cm,
      };
  const UNIT_LABELS = { speed: U.speed, temp: U.temp, press: U.press, travel: U.lenUnit, radius: imperial ? 'in' : 'cm' };

  function translatePage() {
    document.documentElement.lang = lang;
    if (lang === 'en') {
      document.querySelectorAll('[data-t]').forEach(el => { el.textContent = t(el.textContent.replace(/\s+/g, ' ').trim()); });
    }
    document.querySelectorAll('[data-t-ph]').forEach(el => { el.placeholder = t(el.dataset.tPh); });
    document.querySelectorAll('[data-u]').forEach(el => { el.textContent = UNIT_LABELS[el.dataset.u] ?? el.textContent; });
  }

  window.I18N = { lang, imperial, t, U, locale: lang === 'de' ? 'de-DE' : 'en-US', translatePage, EN };
})();
