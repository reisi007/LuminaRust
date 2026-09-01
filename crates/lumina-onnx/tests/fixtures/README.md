# ONNX behavior fixture (`lumina-crafted-reducemax.onnx`)

Dieses Binär-Fixture ist der **hash-gepinnte, committete** Behavior-Artefakt für
den echten ONNX-Runtime-Pfad (`onnx-rt`). Es ist bewusst **kein** echtes
Segmentierungsmodell (keine BiRefNet-/SAM-2-Gewichte, keine Downloads) —
es dient ausschließlich der Verhaltensabsicherung des `OrtBackend`
(Tensor-Namen, Hash-Pin, Output-Validierung, Inferenz) mit einem garantiert
ladbaren, reproduzierbaren Graph.

## Graph

```
x: float[1,3,H,W] ──ReduceMax(axes=[1], keepdims=1)──▶ y: float[1,1,H,W]
```

- Eingang `x`: float NCHW, Shape `[1, 3, 8, 8]`
- Ausgang `y`: float, Shape `[1, 1, 8, 8]`
- Operatorset 13 (attributbasierte `axes`-Form), `ir_version` 8
- `ReduceMax` über die Kanalachse mit `keepdims=1` macht aus dem NCHW-RGB-
  Eingang exakt die Matte-Form, die `OrtBackend` erwartet.

## Provenance (F-073)

- **Programmatisch erzeugt** — reine Proto3-Wire-Encoding aus der
  dokumentierten Quell-of-Truth-Encoder-Logik
  (`crates/lumina-onnx/tests/ort_backend.rs`, `crafted_onnx_bytes()` und
  Helfer); kein Herunterladen, kein fremdes Modell, keine Modellgewichte.
- **Keine Lizenzpflicht**: triviale, formelhaft generierte Graphen-Bytes
  (eigene Erzeugung). Ein Regenerierungs-Skript liegt unter
  `scripts/regenerate_onnx_fixture.sh` (kompiliert den Standalone-Encoder und
  schreibt identische Bytes).

## Pin

- SHA-256 (Datei-Bytes): `2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d`
- Der Test `pinned_fixture_hash_matches_documented_pin` verifiziert diese Pin
  gegen die committeten Bytes; der `OrtBackend`-Test lädt das Fixture mit
  `model_hash` = Pin und erwartet `ModelHashStatus::Verified`.

## Regeneration

`scripts/regenerate_onnx_fixture.sh` schreibt das Fixture deterministisch neu
und prüft, dass die Bytes mit dem Pin übereinstimmen. Ändert sich die
Encoder-Logik in `tests/ort_backend.rs`, MUSS das Fixture neu erzeugt und der
Pin in dieser README sowie in `tests/ort_backend.rs` aktualisiert werden —
ein Drift wird von den Pin-Tests als Fehler gemeldet (kein stiller Fallback).