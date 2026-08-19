# Produktname — Brainstorm

**Status:** Brainstorm-Phase (offen bis MVP-Entscheidung)
**Entscheidung:** Beim MVP wird der finale Name festgelegt.
**Hinweis:** "Luminar" ist ein bestehendes Produkt (Skylum) — Kollision vermeiden.

## Anforderungen

- Kurz, einprägsam, gut aussprechbar (DE + EN)
- Assoziation mit Licht, Bild, RAW, Klarheit
- Keine Kollision mit bestehenden Produkten (Luminar, Lightroom, Aperture, Capture One, DxO …)
- Domain-fähig (idealerweise `.app`, `.io`, `.dev` oder `.rs` verfügbar)
- Funktioniert als CLI-Befehl (`lumina …` oder `<name> …`)
- Funktioniert als Markenname

## Kandidaten

### Lumina-Familie (Licht-Assoziation)

| Name | CLI | Notiz | Kollision? |
|------|-----|-------|------------|
| **Lumina** | `lumina` | Sauber, kurz, wissenschaftlich. Guter Retro-Fit auf den bisherigen Projektnamen. | Lumina (Chevrolet), Lumina (div. SaaS) — mittel |
| **Lumen** | `lumen` | Lateinische Basiseinheit der Lichtstärke. Minimalistisch. | Lumen (div. Apps, Smart-Home) — mittel |
| **Lux** | `lux` | Lateinisch „Licht", 5 Buchstaben, sehr kurz. | Lux (div. Spiele, Tools) — hoch |
| **Lucid** | `lucid` | „Klar, hell". Gute Assoziation für einen Editor. | Lucid (div. SaaS, VR-Headset) — mittel |
| **Lumi** | `lumi` | Verkürzt, freundlich, modern. | Lumi (div. Apps) — mittel |
| **Lumin** | `lumin` | Zwischen Lumina und Lumen. Etwas technischer. | Kollisionen selten — gut |
| **Lumos** | `lumos` | „Licht" (lat.). Kulturell bekannt (Harry Potter). | Lumos (div. Startups) — mittel |

### RAW-/Technisch-Familie

| Name | CLI | Notiz | Kollision? |
|------|-----|-------|------------|
| **Rawly** | `rawly` | Spielerisch, RAW-Bezug offensichtlich. User-Vorschlag. | Kollisionen selten — gut |
| **Rawlight** | `rawlight` | RAW + Light. Beschreibend. | Keine Kollision — sehr gut |
| **Unraw** | `unraw` | Aktion „RAW wird entschlüsselt". Direkt. | Kollisionen selten — gut |
| **Demosaic** | `demosaic` | Technischer Fachbegriff (CFA-Interpolation). Nischenhaft. | Demosaic (div. Libs) — mittel |
| **Firstpixel** | `firstpixel` | „Das erste Pixel zählt". poetisch. | Keine Kollision — sehr gut |

### Licht-/Bild-Familie

| Name | CLI | Notiz | Kollision? |
|------|-----|-------|------------|
| **Photon** | `photon` | Physikalisch, „ Lichtquant". Starke Assoziation. | Photon (div. Games, SaaS) — hoch |
| **Prism** | `prism` | Lichtbrechung, Spektrum. | Prism (NSA-Skandal, div. Tools) — problematisch |
| **Raye** | `raye` | Strahl + weiblich/kreativ. Elegant. | Raye (div. Apps) — mittel |
| **Beam** | `beam` | Strahl, direkt. | Beam (Microsoft, div.) — hoch |
| **Glow** | `glow` | Sanftes Leuchten. Freundlich. | Glow (div. Beauty-Apps) — hoch |

### Komposita / Kreativ

| Name | CLI | Notiz | Kollision? |
|------|-----|-------|------------|
| **Brightroom** | `brightroom` | Wortspiel mit Lightroom. Kühn. | Keine direkte Kollision — aber rechtlich riskant |
| **Clearshot** | `clearshot` | Klarheit + Fotografie. | Clearshot (div. Kamera-Apps) — mittel |
| **Lightcraft** | `lightcraft` | „Licht-Handwerk". Handwerklich. | Kollisionen selten — gut |
| **Exposure** | `exposure` | Kernkonzept der Fotografie. | Exposure (Mac-App) — hoch |
| **Chroma** | `chroma` | Farbe. Gut für Farbbearbeitung. | Chroma (div. Tools) — mittel |
| **Spectrum** | `spectrum` | Spektrum, Farbbereich. | Spectrum (div. Apps, Discord) — hoch |
| **Radiant** | `radiant` | Strahlend. Positiv. | Radiant (div. Foto-Tools) — mittel |

### Kurz & disruptionsfreundlich

| Name | CLI | Notiz | Kollision? |
|------|-----|-------|------------|
| **Flair** | `flair` | Ausdruck, Stil. | Flair (div. SaaS) — mittel |
| **Vivid** | `vivid` | Lebendig, satt. | Vivid (div. Apps) — hoch |
| **Clarity** | `clarity` | Klarheit, Transparenz. | Clarity (div. SaaS) — hoch |
| **Lume** | `lume` | Kurz, „Leuchte". Moderne Sprache. | Lume (div. Startups) — mittel |
| **Nox** | `nox` | „Nacht" (lat.) — Kontrast zum Licht-Thema. Nischenhaft. | Nox (div. Spiele) — mittel |

## Favoriten (Build-Agent-Vorschlag)

1. **Lumina** — stärkste Verbindung zum bisherigen Projektnamen, kurzer CLI-Befehl, gut aussprechbar. Kollisionen handhabbar (andere Domäne).
2. **Rawly** — frisch, einprägsam, RAW-Bezug direkt im Namen. Keine Kollisionen.
3. **Lumin** — technisch-elegant, kurz, wenige Kollisionen.
4. **Rawlight** — beschreibend,复合, klar./null Kollisionen.
5. **Firstpixel** — poetisch, kein Marketing-Namen, kein Kollision.

## Offene Fragen

- Soll der Name „LuminaRust" als EntwicklungsbName beibehalten werden bis zur Entscheidung?
- Ist ein Rust-Bezug im Produktnamen erwünscht oder bewusst vermieden (Technologie-Hiding)?
- Domain-Verfügbarkeit prüfen vor finale Entscheidung.
- Trademark-Recherche vor MVP-Release.

## Nächste Schritte

- [ ] Name bis MVP-Release erweitern (neue Ideen aus User-Feedback)
- [ ] Domain-Verfügbarkeit für Top-3 prüfen
- [ ] Trademark-Clash-Check (US, EU, CN)
- [ ] Beim MVP finale Entscheidung dokumentieren und alle Referenzen aktualisieren
