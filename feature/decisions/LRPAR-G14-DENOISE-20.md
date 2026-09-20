# KI-Denoise — Modell-, Lizenz- und Capability-Entscheid (Doku-first)

**Task:** LRPAR-G14-DENOISE-20 (Release 2.0, G-14-Abspaltung, User-Entscheid 2026-09-03)
**Status:** Entscheid dokumentiert (Doku-first, kein Code) — Abnahme „Entscheid in `feature/` + Folge-Task"
**Bezug:** G-14 Ist ~15 % (`.goal/Goal.md`); manuelles NR F-096 + Schärfen F-095 bleiben MVP; `feature/architecture/pipeline.md` wird hier bewusst **nicht** geändert (nur gelesen)
**Normativ für:** spätere Implementierung (Schema, Pipeline-Stufe, ONNX-Modell, Persistenz)

## 1. Entscheidung (Kurzform)

1. **MVP bleibt manuell:** F-096 (deterministisches CPU-NR, Luminanz/Chroma `0..=1`) und F-095 (Unsharp-Schärfen danach) sind die einzige MVP-Rauschreduzierung. KI-Denoise ist eine **optionale, additive Erweiterung** für Release 2.0 — kein Ersatz, kein stiller Wechsel des Render-Ergebnisses.
2. **Modellweg (F-078-Gate):** KI-Denoise läuft ausschließlich über den nativen, austauschbaren ONNX-Adapter (`lumina-onnx`, Feature `onnx-rt`, native-only). Es werden **keine Gewichte committet und kein Modell gebündelt**, bevor Lizenz (Provenienz + Redistribution), Hash-Pin (`sha256:<hex>`), Input-Spec-Digest und F-078-Audit-Eintrag (`feature/quality/fixtures-licensing.md` §5 + `THIRD-PARTY-NOTICES.md`) vorliegen. Bis dahin trägt das Manifest `model_hash = "pending-integration"`.
3. **Capability:** neue, getrennte Capability `denoise` (lokal ONNX) — getrennt von `inpaint`/`generative_canvas`/`subject_segmentation` und von Cloud-Denoise (Cloud nur mit eigener, expliziter Capability-Entscheidung; nie stiller Fallback).
4. **Rezept/Persistenz:** neues additives Schema-v2-Feld `recipe.adjustments.denoise_ai` (Version 1, s. §5) mit Modellidentität + Artefaktverweis; RGB-Ergebnis-Artefakt im `.lumina.zdata`-Bundle (`kind = "denoise_rgb"`); Identität/Veraltung analog AI-Masken (§6).
5. **Kein stiller Fallback (§6):** fehlendes/veraltetes Modell oder Artefakt → sichtbarer Status (`unavailable`/`stale`/`missing`/`corrupt`), Render fällt **sichtbar** auf manuelles F-096-NR zurück bzw. bricht bei expliziter Anforderung hart ab — niemals stilles Ummappen.

## 2. Abgrenzung zu manuellem NR (F-095/F-096 — MVP bleibt manuell)

- **F-096 (normativ, `pipeline.md`):** `recipe.adjustments.noise_reduction` (`version`, `luminance`/`color` je `0..=1`, 0 = Identität), deterministischer kantenbewusster 5×5-Mittelwert, CPU, reproduzierbar. Liegt in `Adjustments` **vor** Schärfen. „KI-Denoise ist nur eine optionale spätere Erweiterung und wird hier nicht spezifiziert."
- **F-095 (normativ):** `recipe.adjustments.sharpening` (`amount`/`radius`/`detail`/`masking`), Unsharp-Mask, liegt **nach** Rauschreduzierung.
- **Konsequenz für KI-Denoise:**
  - MVP-Rezepte ohne `denoise_ai` rendern byte-identisch wie heute (neues Feld = `None` = Identität, additives Pre-MVP-Muster, keine Migration bestehender Rezepte nötig — Schema-Bump-Regel für verschachtelte Felder bleibt: Nutzung erfordert `recipe_schema_version` 2, s. `pipeline.md` „Schema-Migration (Pre-MVP)").
  - Manuelles F-096 bleibt immer verfügbar und ist der sichtbare Fallback-Anker: Ist KI-Denoise nicht verfügbar/veraltet, gilt das **manuelle** NR-Ergebnis — explizit im Status ausgewiesen, nicht als „gleichwertig" verschleiert.
  - Reihenfolge-Ziel (erst bei Implementierung normativ in `pipeline.md` zu verankern): `… → DenoiseAI (optional) → NoiseReduction (F-096, manuell) → Sharpening (F-095) → …`. KI-Denoise ersetzt F-096 nicht, sondern läuft davor; beide auf 0/`None` = Identität.

## 3. Modell-/Lizenz-Entscheid (F-078-Bezug)

Stand: **keine Modellfestlegung, keine Gewichte, kein Download** — dieser Abschnitt entscheidet das Verfahren und die Kandidatenklasse, nicht das konkrete Gewicht.

- **Verfahren (verbindlich, aus `fixtures-licensing.md` §§5–6/8 + Agents.md-Änderungsregeln):**
  1. Kandidatenfamilie wählen (s. §4) → konkrete Checkpoint-Quelle (Upstream-Repo + Commit/Tag + Datei) benennen.
  2. Lizenz an der Gewichtsquelle verifizieren (Code-Lizenz ≠ Gewichts-Lizenz; AGPL-/NC-Fallen wie `ultralytics`-Pfad bei SAM 2 oder non-commercial-Denoise-Checkpoints schließen eine Bündelung aus — vgl. `fixtures-licensing.md` §5 „SAM-2-Export-Pfad").
  3. Erst danach: Hash-Pin (`sha256` über exakte `.onnx`-Bytes), `input_spec_digest`, Version, Quell-URL und Lizenz in `fixtures-licensing.md` §5 + `THIRD-PARTY-NOTICES.md` erfassen; Manifest-`model_hash` von `pending-integration` auf Pin umstellen.
  4. Tests nur gegen lokale, hash-gepinnte Fixtures (kein Netz, kein spontaner Download); echte Gewichte bleiben `pending-integration` bis Schritt 3 abgeschlossen ist.
- **Lizenz-Risiko (offen, Teil der Folgearbeit):** Viele SOTA-Denoise-Checkpoints (Restormer/NAFNet/SCUNet-Varianten aus Community-Repos) haben unklare oder forschungs-/NC-gebundene Gewichtsbedingungen; ein Apache-2.0-/MIT-konformer ONNX-Exportweg ist Zulassungsbedingung (Analogie: SAM-2-Regel in `fixtures-licensing.md` §5). Fällt kein Kandidat durch das F-078-Gate, bleibt Release 2.0 bei manuellem NR — das wird dann als Scope-Entscheid dokumentiert, nicht stillschweigend umgangen.
- **ORT-Redistribution:** `ort =2.0.0-rc.13` (gepinnt) lädt Prebuilt-Binaries zur Build-Zeit (Netz) — Freigabe des `onnx-rt`-Pfads für 2.0 im Rahmen von F-078-R4 erneut prüfen.

### 3.1 Modellfreigabe-Entscheid (LRPAR-G14-DENOISE-IMPL-20, 2026-09-16)

**Ergebnis der Kandidatenprüfung (Recherche, kein Download, keine Gewichte):**

| Kandidatenfamilie | Code-Lizenz (Quelle) | Gewichts-Lizenz | F-078-Gate |
| --- | --- | --- | --- |
| **DnCNN**-artig (KAIR/DnCNN, Zhang et al.) | MIT (cszn/DnCNN, cszn/KAIR) | Gewichte werden über `main_download_pretrained_models.py` von Drittservern geladen; **keine** explizite Gewichts-Lizenzgewährung im Repo | **nicht freigegeben** (Gewichts-Provenienz fehlt) |
| **NAFNet**-artig (megvii-research/NAFNet, ECCV 2022) | MIT (Repo `LICENSE`), BasicSR-Anteile Apache-2.0 | Pretrained-Modelle nur über Google-Drive/Baidu-Links, **keine** separate Gewichts-Lizenzdatei | **nicht freigegeben** (Gewichts-Provenienz fehlt) |
| **Restormer/SCUNet**-artig (swz30/Restormer, cszn/SCUNet) | MIT (Repo `LICENSE.md`) | Gewichte als GitHub-Release-Artefakte, **keine** explizite Gewichts-Lizenzgewährung | **nicht freigegeben** (Gewichts-Provenienz fehlt) |

**Entscheid:** Kein Kandidat erfüllt das F-078-Gate vollständig, weil die
Code-Lizenz permissiv ist, aber für die **Gewichte** keine explizite,
redistributionsfähige Lizenzgewährung an der Gewichtsquelle vorliegt
(Analogie zur SAM-2/`ultralytics`-Falle in `fixtures-licensing.md` §5). Daher
gilt bis auf Weiteres der **Fixture-Weg wie GEN-ONNX-1**:

1. Release 2.0 nutzt ein **deterministisches, hash-gepinntes Fixture-Modell**
   ohne Gewichte und ohne Netz (programmatisch erzeugter ONNX-Graph,
   `model_hash` real gepinnt) und lehnt **echte Gewichte laut** ab
   (`model_hash = "pending-integration"` → Status `unavailable`, never silent).
2. Erst wenn Quelle + Gewichts-Lizenz + Hash-Pin + `input_spec_digest` +
   F-078-Audit-Eintrag (`feature/quality/fixtures-licensing.md` §5 +
   `THIRD-PARTY-NOTICES.md`) vorliegen, wird von `pending-integration` auf den
   echten Pin umgestellt. Dieser Schritt ist **nicht** Teil des Kern-Slices.
3. Der Core-Slice (Pipeline-Blend, Statusmodell, zdata-`denoise_rgb`) ist
   modellunabhängig und benötigt kein ONNX zum Testen — Tests laufen gegen
   synthetische, deterministische RGB-Artefakte ohne Netz.

**Lizenztext-Vorschlag (nur Meldung, keine Änderung in diesem Slice):** Sobald
ein Kandidat das Gate passiert, gehört in `fixtures-licensing.md` §5 eine
Zeile der Form „`<Modell>` | KI-Denoise (RGB) | **MIT** (`<Upstream-Repo>`
`LICENSE`, Gewichte unter derselben Lizenz — verifiziert `<Datum>`) |
Gewichte *pending integration* (`model_hash = "pending-integration"`)" plus der
`THIRD-PARTY-NOTICES.md`-Eintrag mit `sha256:`-Pin und Quell-URL.

## 4. ONNX-Einordnung (Modellfamilie, Auflösung, Vorverarbeitung)

- **Modellklasse:** RGB-zu-RGB-Denoiser (kein Masken-/Matten-Modell, kein generatives Canvas): Eingang sRGB-codiertes RGB(A)-Raster, Ausgang entrauschtes RGB in gleicher Geometrie. Kandidatenfamilien (alle erst nach F-078-Prüfung zulässig): **DnCNN-artige** CNN-Baseline (klein, schnell, schwache High-ISO-Leistung), **UNet/NAFNet-artige** Encoder-Decoder (ausgewogen, Favorit für v1), **Restormer/SCUNet-artige** Transformer (beste Qualität, schwer, ggf. nur Desktop-GPU/CPU-langsam). Keine Festlegung hier — die Folgearbeit evaluiert genau einen v1-Kandidaten gegen Golden/PSNR-Gates.
- **Auflösung:** Tiled Full-Resolution-Inferenz (kein 1024-Downscale wie BiRefNet/SAM 2 — Denoise muss Vollauflösung bedienen): inferiert wird in Kacheln (z. B. 512×512 mit Overlap-Blend gegen Kantenartefakte; exakte Kachel/Overlap-Werte legt die Implementierung fest) direkt auf dem Decode-Frame. Kachelgröße, Overlap und Blend-Verfahren sind Teil des `input_spec_digest` — jede Änderung invalidiert persistierte Artefakte sichtbar.
- **Vorverarbeitung:** pro Manifest `ModelInputSpec` (Analogie `ai-masks.md` §Maskenidentität): Normalisierung (vorzugsweise Identität/`[0,1]`-Skalierung statt ImageNet-mean/std — Denoiser sind in der Regel nicht ImageNet-normalisiert; Festlegung mit Modellwahl), Kanal-Layout `Rgb`, Tensor-Format `Nchw`, dokumentierte Tensor-Namen. Nachskalierung: Identität (kein Upsampling — Ausgabegeometrie = Eingabegeometrie; anders als Masken-Resample).
- **Farbraum-Hinweis:** MVP-Pipeline arbeitet sRGB-codiert (`Rgba8Srgb`); KI-Denoise v1 arbeitet ebenfalls dort. Ein linearer Denoise-Pfad ist erst mit einer linearen Pipelineversion zulässig (getrennt migriert/validiert, kein stiller Wechsel).

> **Umsetzungs-Nachtrag ONNX-Slice (LRPAR-G14-DENOISE-IMPL-20, 2026-09-16):**
> Die in §4 skizzierte ONNX-Einordnung ist im `lumina-onnx`-Backend umgesetzt:
> additive, getrennte `denoise`-Capability (`ModelCapabilities.denoise`, Muster
> `face_detect`/`face_embed`), Deskriptor + `DenoiseModelSuite` mit
> `DenoiseTileSpec`, deterministischer `input_spec_digest` (Modell-Input-Spec
> **plus** Kachelgröße/Overlap, Identitäts-/`[0,1]`-Vorverarbeitung und
> Identitäts-Nachskalierung), Fixture-Pin nach GEN-ONNX-1 (§3.1) sowie der echte
> ORT-Pfad hinter `onnx-rt` (SHA-256-Verifikation, Tensor-Namen-Gates,
> `ModelArtifactStale` bei Hash-Abweichung). Ein `pending-integration`-Manifest
> wird dort **laut** als `ModelUnavailable` abgelehnt — das ist der sichtbare
> §6-Status `unavailable`, nie ein stiller Stub-Ersatz. Der Producer inferiert
> gekachelt und assembliert nahtlos über den Core-Vertrag
> `assemble_denoise_tiles`; die Produzenten-Herkunft wird über
> `set_denoise_producer_provenance` in `DenoiseAi.extras` persistiert. Die
> konkreten Werte (Default 512×512, Overlap 32, keine Reskalierung) sind Teil
> des `input_spec_digest` und damit invalidierungsrelevant.

## 5. Rezept-Stufen-Vorschlag + Persistenz (noch nicht normativ)

Vorschlag für die Implementierungs-Folgearbeit (erst dort nach `sidecar.md`-Vertrag zu normieren; `pipeline.md` bleibt bis dahin unverändert):

```jsonc
// additives Schema-v2-Feld, None = Identität (MVP-Rezepte unverändert)
"denoise_ai": {
  "version": 1,
  "enabled": true,
  "model": { "name": "<z.B. nafnet-srgb>", "version": "<tag>", "model_hash": "sha256:<hex>" },
  "input_spec_digest": "sha256:<hex>",   // Auflösung/Kachel/Overlap/Norm/Tensor-Namen
  "strength": 0.5,                        // 0..=1, 0 = Identität (nur Blending, keine Inferenz nötig)
  "preserve_detail": 0.5,                 // 0..=1, Kanten-/Detailschutz
  "artifact": { "path": "<relativ im Bundle>", "sha256": "<hex>", "kind": "denoise_rgb" }
}
```

> **Umsetzungs-Nachtrag (Schema-Slice 2026-09-16, verifiziert BESTANDEN):**
> Die Skizze oben war Kurzform. Umgesetzt ist die volle `DenoiseArtifactRef`
> nach Sidecar-Artefaktvertrag (`relative_path`, `format`, `checksum` =
> BLAKE3 über den unkomprimierten RGB-Strom, `width`/`height`, `channels`,
> `data_version`, `kind = "denoise_rgb"`); der zdata-`RecordKind`
> (`RecordKind::DenoiseRgb = 4`) ist im Kern-Slice implementiert
> (Codec + atomarer Write unter `.zdata.lock`, s. `sidecar.md`).
> Diese Form ist normativ, die Skizze nur illustrativ.

- **Validierung:** `version == 1`, `strength`/`preserve_detail` endlich in `0..=1`, Modell-Identität vollständig (sonst Ablehnung, kein Clipping/Ergänzen); unbekannte Felder bleiben erhalten (Roundtrip-Regel).
- **Artefakt:** entrauschtes RGB liegt als versionierter Eintrag im `.lumina.zdata`-Bundle (`kind = "denoise_rgb"`, relativer Pfad, Format/Auflösung/Kanäle/Prüfsumme, atomarer Write, `.zdata.lock`-Serialisierung — Muster: `sidecar.md` + `generative-expand.md`/`spot-removal.md`). Kein unkomprimiertes Float-Array im JSON. Pro virtuelle Kopie referenziert (Teilung auf Quellebene zulässig wie bei Masken-Matten; Layer/Invert/lokale Anpassung bleibt Kopien-Sache).
- **Render-Key:** `denoise_ai`-Inhalt (inkl. `input_spec_digest`, Modell-Hash, Artefakt-Hash) fließt in `recipe_hash`/Render-Key ein; Änderung invalidiert ab dieser Unterstufe (nicht Decode/AI-Masken).
- **Determinismus:** gleiche Eingaben (Quelle, Decode-Kontext, Modell-Hash, Input-Spec, Strength/Detail) → byte-identisches Artefakt (BLAKE3 über den unkomprimierten RGB-Strom).

## 6. Kein stiller Fallback (Statusmodell)

Analog AI-Masken (`Valid`/`stale`, F-048/F-051) und Spot-Remove/GenerativeEdit:

| Lage | Sichtbares Verhalten |
| --- | --- |
| Modell fehlt (`pending-integration`, kein Pfad) | Status `unavailable` + Warnung; Render nutzt manuelles F-096 (falls gesetzt) bzw. Identität — **ausgewiesen**, CLI-Exit 0 mit `stderr`-Warnung, GUI-Badge |
| Artefakt fehlt/korrupt | `missing`/`corrupt`, harter Hinweis; Neuberechnung nur explizit (Button/Flag), nie automatisch als einzige Option |
| Quelle/Decode/Modell/Input-Spec geändert | Artefakt `stale`, Wiederverwendung verboten; explizite Re-Inferenz oder manuelles NR |
| Stärke 0 / `enabled: false` / Feld `None` | Identität ohne Inferenz (kein Modell nötig, kein Fehler) |

Gültigkeit verlangt Übereinstimmung von Quell-Hash, Decode-/Geometrie-Kontext, Modellkontext (Name/Version/Hash), `input_spec_digest` und Artefakt-Prüfsumme. `MaskPolicy`-Analogie (`Strict` vs. `Warn`) übernimmt die Folgearbeit.

> **Umsetzungs-Nachtrag (Rework 2026-09-16, B2/B3):** `DenoiseAi` und das
> zdata-`DenoiseRgbArtifact` persistieren selbst keine Produzenten-Herkunft.
> Die `recorded`-Identität wird daher additiv über den geflatteten
> `DenoiseAi.extras`-Schlüssel `producer_identity` gespeichert und
> zurückgelesen (`set_denoise_producer_provenance` /
> `denoise_producer_provenance`, `lumina-core`); das ist roundtrip-legal und
> braucht keine Schema-/Migrationsänderung. `resolve_denoise_status` vergleicht
> nun **alle** `recorded`-Felder mit dem Live-Kontext und meldet jede
> Abweichung (inkl. `model_version`, `input_spec_digest`,
> `artifact_checksum`) als `stale`; eine fehlende/kaputte Provenienz ist nie
> still `ready`. Der GPU-Pfad ist bis zur WGSL-Stufe nicht verdrahtet: ein
> aktives `denoise_ai` wird als `denoise_ai (not GPU-wired)` auf die CPU
> geroutet und bricht dort laut ab (§6, kein stiller Fallback).

### 6.1 Sample-Sidecar-Zustand (R5-DENOISEFIX-22, 2026-09-20)

**Befund:** Das Sample-Sidecar `sample-data/raw/aircraft-landscape.cr3.lumina.json`
trug ein **aktives** `denoise_ai` (`enabled: true`) mit
`model_hash = "pending-integration"` — sowohl im aktiven Rezept als auch im
`history`-Snapshot. Nach §3.1/§6 ist das der sichtbare `unavailable`-Fall:
Der Recipe-Gate routet `denoise_ai (not GPU-wired)` auf die CPU (GUI-Badge),
der Full-Res-Pfad bricht unter der Default-`Strict`-Policy laut ab. Der
manuelle Akzeptanz-Run F-103-N6 Runde 6 belegte das mit 5× ERROR
`no denoise artifact resolved` + 5× Recipe-Gate-Warn.

**Fix (fixture-/zustandsseitig, kein Code):** `enabled` wurde im **aktiven
Rezept und im `history`-Snapshot** auf `false` gesetzt. Beide Stellen müssen
übereinstimmen, weil ein History-Revert den Render byte-identisch
reproduzieren soll (`history_recipe_snapshot_reproduces_the_original_render`).
`enabled: false` ist per `DenoiseAi::is_identity()` reine Identität — kein
Modell, kein Artefakt, kein Fehler; `lumina-gpu::unsupported_gpu_stages_with_context`
listet die Stufe dann nicht mehr als CPU-Routen-Grund. Der G-1-WGSL-Pass
bleibt unangetastet (kein Code-Strip), es gibt weiterhin keinen stillen
Fallback.

**Randbedingung (wichtig für Verifikation und Folgearbeit):** Die Datei ist
**nicht committet**. `*.lumina.json` steht in `.gitignore` („User sidecars
(per-image edit recipes) are never committed — local only"); `git ls-files`
und `HEAD` kennen sie nicht. Das Sample-Sidecar ist damit ein **lokales,
generiertes** Artefakt aus manuellen GUI-Läufen (hier: ein persistierter
Denoise-Toggle). Die Richtigstellung wirkt für den lokalen manuellen Test,
ist aber nicht über einen Commit dauerhaft; ein erneutes Einschalten von
Denoise im Sample-Ordner stellt den Zustand wieder her. Ein dauerhafter
Repo-Sample-Zustand wäre eine eigene Entscheidung (Sidecar bewusst als
Fixture committen **oder** einen deterministischen Test-Sidecar außerhalb des
Sample-Ordners führen) — kein stilles Umgehen des Ignore.

## 7. Abgrenzung Rote Augen (nicht hier gelöst)

G-14 ist in zwei Tasks gespalten (Releaseplan, User-Entscheid 2026-09-03): **Rote-Augen-Korrektur → LRPAR-G14-REDEYE-15 (Release 1.5)**, KI-Denoise → dieser Entscheid (Release 2.0). Rote Augen (Erkennung + Korrektur als eigene Rezept-Stufe) wird hier bewusst **nicht** spezifiziert; Querverweis genügt. Gemeinsame F-078-/Capability-Muster dürfen wiederverwendet werden, die Stufen bleiben getrennt.

## 8. Folge-Tasks (Vorschlag für `Agents.todo.md`, Build-Agent entscheidet)

1. **F-078-Modellfreigabe Denoise:** Kandidat wählen, Gewichts-Lizenz verifizieren, Hash + Input-Spec pinnen, `fixtures-licensing.md` §5 + `THIRD-PARTY-NOTICES.md` ergänzen (Gate für alles Weitere).
2. **Schema + Persistenz:** `denoise_ai`-Feld (additiv, v2), `kind = "denoise_rgb"` in zdata, JSON-Roundtrip-/Migrations-/Atomic-Write-/Recovery-Tests.
3. **Pipeline-Stufe:** Einordnung `DenoiseAI → F-096 → F-095` in `pipeline.md` normieren (Version, Reihenfolge, Render-Key), Core-Implementierung + Golden/PSNR-Gates (u. a. `strength: 0` = Identität, Determinismus, Kachel-Blend-Nahtlosigkeit).
4. **ONNX-Verdrahtung:** `denoise`-Capability in Manifest + Capability-Matrix (`platform/capability-matrix.md`, native CLI/Desktop, Cloud explizit nicht geplant), `lumina-onnx`-Backend, Veraltungs-/Statusmodell aus §6, CLI/GUI-Anbindung (Fehler laut, Badges). Auflagen aus der Kern-Verifizierung (2026-09-16, mitzuziehen): CPU-seitiger `CoreError::Denoise{invalid}`-Mapping-Test (F3, 11 Fälle; `format`/`channels`/`data_version` offen), `recorded`-Aufruferkonvention (`None` → `DenoiseIdentity::default()`, R1), CLI wählt explizit `DenoisePolicy::Warn` für §6-Exit-0 (R2), Cross-Crate-Checksum-Test Core↔Sidecar (B4, umgesetzt). Folgearbeit aus der ONNX-Verifizierung (2026-09-16): Blend-/Assembly-Version in den `input_spec_digest` aufnehmen (F1 — derzeit flippt eine Gewichtungsänderung den Digest nicht; folgenlos solange `pending-integration`, aber SOLL-Abweichung zu §4).

   > **Status ONNX-Slice (2026-09-16):** `denoise`-Capability + Deskriptor +
   > `DenoiseModelSuite`/`DenoiseTileSpec` + Fixture-Pin (§3.1) +
   > `input_spec_digest`-Produzent + ORT-Pfad (`onnx-rt`: SHA-256-Gate,
   > Tensor-Namen-Gates, `pending-integration` → `ModelUnavailable`) +
   > deterministische, tests-only Stubs + gekachelter Producer über
   > `assemble_denoise_tiles` + Produzenten-Provenienz sind umgesetzt und
   > getestet (Capability-/Hash-Gates, Stub-Determinismus, Modellwechsel →
   > `stale`, `unavailable` ohne stillen Fallback, Kachel-Nahtlosigkeit).
   > Die Auflagen F3 (Core-Mapping-Test) und B4 (Core↔Sidecar-Checksum) sind
   > umgesetzt. Offen und damit weiter in `Agents.todo.md`: Capability-Matrix-
   > Eintrag (native CLI/Desktop, Cloud explizit nein), CLI-Anbindung
   > (`DenoisePolicy::Warn`, Exit 0, `stderr`), GUI-Badges/Panel, R1/R2-
   > Aufruferkonvention und Perf-Budgets (Punkt 5).
5. **Perf-Budgets:** Denoise-Benchmarks nach `performance-benchmarks.md` (F-074), Budgets im selben Commit wie das Feature begründen.
   > **Status Perf-Slice (F-074-N8, 2026-09-17):** Umgesetzt. Die Denoise-Klasse
   > (`denoise/blend__*`, `denoise/assemble_tiles__*`, `denoise/render_ready__*`,
   > `denoise/status_resolve__ready`) ist in `crates/lumina-bench/bench/denoise.rs`
   > implementiert und in `perf/baseline.json`/`perf/budgets.json` registriert
   > (report-only, `gate: false`, `budget_ns` ≈ 2× Median). Da die Gewichte
   > `pending-integration` sind (§3.1), ist das **kein Modell-Benchmark**: gemessen
   > wird der deterministische Fixture-Pfad. Details: `performance-benchmarks.md`
   > §F-074-N8.
6. **V1-Modellvergleich (optional):** DnCNN- vs. NAFNet- vs. Transformer-Kandidat an High-ISO-Fixtures messen; Ergebnis als Entscheid-Nachtrag hier dokumentieren.

**Stand 2026-09-16 (Kern + ONNX + CLI + GUI, Verifizierung BESTANDEN):**
Kern-Stufe (GPU-Refusal, Provenienz-`extras`, zdata `kind = 4`), ONNX-Backend
(`denoise`-Capability, `pending-integration` → `unavailable`, B4-Kreuzprobe),
CLI (`--status`/`--render`/`--record-rgb` mutually exclusive, Exit 2 bei
Konflikt; `--render` honoriert `--format`/`--quality` wie `render`; echte
Masken-Ablehnung), GUI (Panel + Badges + Persistenz-E2E). Offen: Gewichte
(S6/F-078), F1-Blend-Digest, F3-Restfelder.

**Stand 2026-09-17 (Perf-Slice F-074-N8, Verifizierung ausstehend):** Die in
§8 Punkt 5 geforderten Denoise-Budgets sind registriert
(`denoise/*`, report-only). Damit ist der Perf-Punkt dieses Entscheids
erledigt; offen bleiben nur noch Gewichte (S6/F-078), F1-Blend-Digest und die
F3-Restfelder.

## 9. Referenzen

- `feature/architecture/pipeline.md` (F-095/F-096, gelesen, nicht geändert) · `feature/quality/fixtures-licensing.md` (F-078/F-073) · `feature/product/ai-masks.md` (Identität/Artefakt-Muster) · `feature/product/generative-expand.md` §Abgrenzung (§KI-Denoise) · `feature/product/spot-removal.md` (Capability-Trennung) · `feature/platform/capability-matrix.md` (native-only ONNX) · `feature/decisions.md` (ONNX-Backend) · `.goal/Goal.md` G-14 · `Agents.todo.md` (LRPAR-G14-DENOISE-20, LRPAR-G14-REDEYE-15).
