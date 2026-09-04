# IPTC-Metadaten (LRPAR-G15-META-15 / LRPAR-G15-IPTC, G-15, Release 1.0, Doku-first)

**Task-ID (Dach):** LRPAR-G15-META-15 · **Sub-Tasks:** LRPAR-G15-IPTC-S1…S8 ·
**Goal:** G-15 Metadaten-Verwaltung · **Release:** 1.0 (MVP; per User-Entscheid
2026-09-04 von 1.5 vorgezogen — vorher: Release-Staffel 2026-09-03) ·
**Stand:** Doku-first (Entscheid, kein Code) ·
**Normatives SOLL:** [`product/iptc-metadata.md`](../product/iptc-metadata.md)

Dieses Dokument ist der verbindliche **Entscheid** für IPTC-Metadaten. Es
legt Speicher, Schreib-Backend, Format-Scope, Presets, Sync, MCP und GUI-Umfang
fest und begründet die verworfenen Alternativen. Das normative Feature-SOLL
(Feld-Registry, Datenmodell, Verträge, Abnahme) steht im verlinkten
SOLL-Dokument. Die Implementierung beginnt erst nach diesem Entscheid; bis
dahin wird **kein Crate-Code** angelegt oder geändert.

## Ziel und Abgrenzung

Lightroom-ähnliche IPTC-Metadatenvergabe: Änderungen an IPTC-Core-Feldern
(Titel, Schlagzeile, Beschreibung, Keywords, Copyright, Creator, Credit,
Source, Location, DateCreated) bleiben als **Entwurf (Draft)** nicht-destruktiv
im Sidecar und werden erst beim Export in die **neu erzeugte** Exportdatei
eingebrannt. Dazu: statische und dynamische Presets, Feld-selektiver Sync,
CLI-, MCP- und GUI-Zugriff.

Nicht-Ziele (explizit):

- Keine Mutierung von Originalen — auch nicht „in place“ mit Backup.
- Keine XMP-/IPTC-**Lesung** als Bearbeitungsquelle (Import-Pfad Post-MVP);
  XMP-Sidecars bleiben verboten (`SidecarError::XmpUnsupported`).
- Kein EXIF-Write (Kamera-/Objektiv-/Aufnahmezeit-Daten) — bleibt Post-MVP
  gemäß `product/export.md`.
- Kein IPTC-Extension-Set, keine Veröffentlichungsdienste (nie Ziel).

## Festgelegte Entscheidungen

1. **Speicher = Sidecar, keine zweite Persistenz.** Der Entwurf liegt als
   neues, additiv-optional Feld `metadata` (Version 1: `draft`-Feldmap +
   **eigene, von der Bearbeitungshistorie getrennte `history`**) auf
   Quellbild-Ebene in `<original>.lumina.json`. Muster: `keywords`/
   `collections` aus G-15-META-MVP-Slice 1 (kein Migrationszwang pre-MVP).
   Keywords selbst werden **nicht dupliziert** — sie bleiben im bestehenden
   `keywords`-Feld; `meta draft set --field keywords` routet dorthin.
2. **Keine SQLite-Datenbank** als Entwurfsspeicher (auch nicht zusätzlich):
   Sidecar-first-Invariante; ein Index wäre nicht-autoritativ und im
   Feature-MVP ohne Mehrwert.
3. **Kein `.xmp`-Sidecar.** Bruch mit „XMP wird in v1 nicht unterstützt“ als
   Sidecar-/Lese-Format. Präzisierung der Entscheidung (2026-09-04): Das
   **Schreiben** von IPTC IIM + XMP in neu erzeugte Exportdateien ist
   ausdrücklich erlaubt und ändert nichts an der Sidecar-Regel.
4. **Schreib-Backend = pure Rust, keine Runtime-Abhängigkeit.** IPTC IIM
   (JPEG `APP13`/8BIM, `1:90` CodedCharacterSet UTF-8) wird handgeschrieben
   (neues Crate `lumina-iptc`); XMP (`APP1`) via pure-Rust-Crate
   `xmp-writer` (Compile-time-Dependency, Lizenzprüfung nach F-073 vor
   Integration, Eintrag in `THIRD-PARTY-NOTICES.md`).
5. **Kein ExifTool.** Weder als Runtime-Abhängigkeit (User-Vorgabe
   2026-09-04) noch gebündelt: Bündeln wäre trotzdem eine Runtime-Abhängigkeit
   (Perl-Runtime, Distributions-/Lizenzlast, CI-Sonderfälle). Der
   Format-Scope wird dafür bewusst auf **JPEG** begrenzt.
6. **Bake-In nur JPEG, nur Opt-in.** `--write-metadata` an
   `export`/`process`/`batch`; PNG/WebP lehnen die Option pro Datei **laut**
   ab (kein stilles Weglassen). Ohne Opt-in bleibt das heutige Verhalten
   (keine Metadaten, keine stillen Annahmen).
7. **GUI gleich mit (User-Entscheid 2026-09-04).** Metadaten-Panel im
   Library-Modul inkl. Preset-Auswahl, Prompt-Dialog für dynamische
   Preset-Variablen und „Auf Auswahl synchronisieren“ — mit Headless-Tests
   (egui Context + LuminaApp, tempdir) und kittest-Golden des Panels.
8. **MCP pfadbasiert + Resources.** Die fünf Tools sind pfadbasiert (Muster
   der F-101-F1-Bulk-Tools, neben der Single-Image-Session); die Resource
   lautet `metadata://draft/<urlencoded-pfad>` — **nicht** `image_id`, weil
   `image_id` prozess-lokal ist und bei Server-Neustart neu nummeriert wird.
   Damit wird die `resources`-Capability eingeführt (Read-only); `prompts`
   bleiben nicht implementiert.

## Verworfene Alternativen (mit Begründung)

- **SQLite-Entwurfsspeicher:** Widerspricht der Sidecar-first-Invariante;
  Sidecar-Wiederherstellbarkeit wäre zusätzlich an einen Rebuild gekoppelt.
- **`.xmp`-Sidecar als Draft:** Doppelte Persistenz-Wahrheit, Bruch mit der
  XMP-Entscheidung, keine atomaren Cross-Format-Transaktionen.
- **ExifTool-Subprozess/Bündel:** Vollere Formatabdeckung (PNG/WebP/TIFF,
  EXIF-Write), aber externe Perl-Runtime, Lizenz-/Distributionslast,
  CI-Sonderfälle — gegen die User-Vorgabe „keine Runtime-Dependency“.
- **GUI erst später:** verworfen — User-Entscheid 2026-09-04 zieht das Panel
  in den Feature-MVP.

## Folge-Implementierungstasks

Vollständige Umfänge und Abnahmekriterien: `Agents.todo.md` (Block A,
LRPAR-G15-IPTC-S1…S8) und SOLL-Dokument. Reihenfolge: S0 (diese Doku) →
S1 ∥ S2 (verschiedene Crates, keine gemeinsamen APIs) → S3–S6 (seriell auf
`lumina-cli`, „ein Crate = ein schreibender Agent“) → S7 (MCP) und S8 (GUI).
Jeder Task: `general`-Implementierungs-Agent + unabhängiger
`general`-Verifizierungs-Agent; kein Commit durch Subagenten.

## Offene Risiken

- **IIM-Oktettlimits vs. Sidecar-Zeichenlimits (mittel):** Gültige Sidecar-
  Werte (Multi-Byte-Text) können die IIM-Oktettgrenzen überschreiten. SOLL:
  lauter per-Datei-Fehler beim Bake-In (Feld + Limit genannt), nie Still-
  Kürzen — UX-Abwägung bleibt im Folge-Task.
- **`xmp-writer`-Lizenz (niedrig):** Apache-2.0/MIT-Annahme ist vor
  Integration gemäß F-073 zu verifizieren und zu dokumentieren; sonst
  Eigenschreibweise (XML-Packet) im Crate `lumina-iptc`.
- **Kompatibilität IIM vs. XMP (niedrig):** Es werden bewusst beide Formate
  geschrieben (IIM für Legacy, XMP für moderne Tools); falls sich in der
  Praxis ein Konflikt zeigt (z. B. abweichende Werte in Readern), ist der
  Nachschärfungs-Task eine Feld-Registry-Anpassung mit Tests, kein Still-
  Fallback.
- **JPEG-only (bewusst):** PNG/WebP-Exporte mit `--write-metadata` scheitern
  laut. TIFF/EXIF-Write und Metadaten-Weitergabe aus der Quelle bleiben
  dokumentierte Post-MVP-Grenzen.
