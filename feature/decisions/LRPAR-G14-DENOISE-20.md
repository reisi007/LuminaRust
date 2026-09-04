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

## 4. ONNX-Einordnung (Modellfamilie, Auflösung, Vorverarbeitung)

- **Modellklasse:** RGB-zu-RGB-Denoiser (kein Masken-/Matten-Modell, kein generatives Canvas): Eingang sRGB-codiertes RGB(A)-Raster, Ausgang entrauschtes RGB in gleicher Geometrie. Kandidatenfamilien (alle erst nach F-078-Prüfung zulässig): **DnCNN-artige** CNN-Baseline (klein, schnell, schwache High-ISO-Leistung), **UNet/NAFNet-artige** Encoder-Decoder (ausgewogen, Favorit für v1), **Restormer/SCUNet-artige** Transformer (beste Qualität, schwer, ggf. nur Desktop-GPU/CPU-langsam). Keine Festlegung hier — die Folgearbeit evaluiert genau einen v1-Kandidaten gegen Golden/PSNR-Gates.
- **Auflösung:** Tiled Full-Resolution-Inferenz (kein 1024-Downscale wie BiRefNet/SAM 2 — Denoise muss Vollauflösung bedienen): inferiert wird in Kacheln (z. B. 512×512 mit Overlap-Blend gegen Kantenartefakte; exakte Kachel/Overlap-Werte legt die Implementierung fest) direkt auf dem Decode-Frame. Kachelgröße, Overlap und Blend-Verfahren sind Teil des `input_spec_digest` — jede Änderung invalidiert persistierte Artefakte sichtbar.
- **Vorverarbeitung:** pro Manifest `ModelInputSpec` (Analogie `ai-masks.md` §Maskenidentität): Normalisierung (vorzugsweise Identität/`[0,1]`-Skalierung statt ImageNet-mean/std — Denoiser sind in der Regel nicht ImageNet-normalisiert; Festlegung mit Modellwahl), Kanal-Layout `Rgb`, Tensor-Format `Nchw`, dokumentierte Tensor-Namen. Nachskalierung: Identität (kein Upsampling — Ausgabegeometrie = Eingabegeometrie; anders als Masken-Resample).
- **Farbraum-Hinweis:** MVP-Pipeline arbeitet sRGB-codiert (`Rgba8Srgb`); KI-Denoise v1 arbeitet ebenfalls dort. Ein linearer Denoise-Pfad ist erst mit einer linearen Pipelineversion zulässig (getrennt migriert/validiert, kein stiller Wechsel).

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

## 7. Abgrenzung Rote Augen (nicht hier gelöst)

G-14 ist in zwei Tasks gespalten (Releaseplan, User-Entscheid 2026-09-03): **Rote-Augen-Korrektur → LRPAR-G14-REDEYE-15 (Release 1.5)**, KI-Denoise → dieser Entscheid (Release 2.0). Rote Augen (Erkennung + Korrektur als eigene Rezept-Stufe) wird hier bewusst **nicht** spezifiziert; Querverweis genügt. Gemeinsame F-078-/Capability-Muster dürfen wiederverwendet werden, die Stufen bleiben getrennt.

## 8. Folge-Tasks (Vorschlag für `Agents.todo.md`, Build-Agent entscheidet)

1. **F-078-Modellfreigabe Denoise:** Kandidat wählen, Gewichts-Lizenz verifizieren, Hash + Input-Spec pinnen, `fixtures-licensing.md` §5 + `THIRD-PARTY-NOTICES.md` ergänzen (Gate für alles Weitere).
2. **Schema + Persistenz:** `denoise_ai`-Feld (additiv, v2), `kind = "denoise_rgb"` in zdata, JSON-Roundtrip-/Migrations-/Atomic-Write-/Recovery-Tests.
3. **Pipeline-Stufe:** Einordnung `DenoiseAI → F-096 → F-095` in `pipeline.md` normieren (Version, Reihenfolge, Render-Key), Core-Implementierung + Golden/PSNR-Gates (u. a. `strength: 0` = Identität, Determinismus, Kachel-Blend-Nahtlosigkeit).
4. **ONNX-Verdrahtung:** `denoise`-Capability in Manifest + Capability-Matrix (`platform/capability-matrix.md`, native CLI/Desktop, Cloud explizit nicht geplant), `lumina-onnx`-Backend, Veraltungs-/Statusmodell aus §6, CLI/GUI-Anbindung (Fehler laut, Badges).
5. **Perf-Budgets:** Denoise-Benchmarks nach `performance-benchmarks.md` (F-074), Budgets im selben Commit wie das Feature begründen.
6. **V1-Modellvergleich (optional):** DnCNN- vs. NAFNet- vs. Transformer-Kandidat an High-ISO-Fixtures messen; Ergebnis als Entscheid-Nachtrag hier dokumentieren.

## 9. Referenzen

- `feature/architecture/pipeline.md` (F-095/F-096, gelesen, nicht geändert) · `feature/quality/fixtures-licensing.md` (F-078/F-073) · `feature/product/ai-masks.md` (Identität/Artefakt-Muster) · `feature/product/generative-expand.md` §Abgrenzung (§KI-Denoise) · `feature/product/spot-removal.md` (Capability-Trennung) · `feature/platform/capability-matrix.md` (native-only ONNX) · `feature/decisions.md` (ONNX-Backend) · `.goal/Goal.md` G-14 · `Agents.todo.md` (LRPAR-G14-DENOISE-20, LRPAR-G14-REDEYE-15).
