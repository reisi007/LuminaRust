# GPU-Render-Nichtdeterminismus: Grenze und Beweisplan (2026-10-16)

**Feature:** F-074 GPU-Bench / `GPU-RENDER-NONDETERMINISM-69`

## Ziel und Geltungsbereich

Dieses Dokument hält die **Grenze** der headless Untersuchung des
GPU-Render-Nichtdeterminismus fest. Es trennt strikt **gemessen**,
**abgeleitet** und **Vermutung** und listet die **Nicht-Behauptungen**
explizit auf. Es ist die dokumentarische Grundlage für die Aufteilung des
Tasks `GPU-RENDER-NONDETERMINISM-69` nach der Regel
`HEADLESS-GPU-TASK-SPLIT` (`Agents.headless.md` §1 Punkt 6).

## Gemessen (read-only, 2026-10-16)

### Adapterlage

- **Kein wgpu-Adapter auf dieser Maschine.** Beleg:
  `cargo test -p lumina-gpu --features gpu-adapter-tests --test stages upload_mask_plane_roundtrip_is_byte_exact -- --exact`
  → Panik in `crates/lumina-gpu/tests/support/mod.rs:45`
  („this test requires a real GPU adapter, but none was bound").
- Zusätzlich: kein `/dev/dri`, kein `/dev/kfd`, kein `vulkaninfo`.

### Roter Test ist headless nicht reproduzierbar

- `cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move -- --exact`
  → **3/3 grün** (`1 passed; 158 filtered out`).
- Grund: ohne Adapter nimmt `render_best_effort`
  (`crates/lumina-cli/src/main.rs:350`) im `_ =>`-Arm (`main.rs:403-410`)
  die CPU-Route: `render_frame_with_generative(...)` →
  `Ok((output, RenderRoute::Cpu { reasons }))`.
- Die grünen Läufe belegen die **CPU-Route**, **nicht** die GPU-Route.

### Keine lokalisierbare Nichtdeterminismus-Quelle im Projekt-Code

Der Auto-Tone-GPU-Pfad ist **synchron/blocking**:
`map_async` + `poll(wgpu::PollType::wait_indefinitely)` + `rx.recv()`
(`crates/lumina-gpu/src/lib.rs:3615-3628`, `gpu_util.rs:96-107`).

Der Pass nutzt `LoadOp::Clear(0,0,0,1)` + `StoreOp::Store` +
Fullscreen-Triangle (`lib.rs:3442-3467`).

Das Readback liest exakt `width*4` Bytes je Zeile und überspringt Padding
(`lib.rs:3633-3638`).

Das GPU-Ressourcen-Pooling ist ein `Mutex<HashMap<…>>` (`lib.rs:810`, `:3234`).

Der Tone-Shader verwendet **keine** Atome (die einzigen `atomicMax` liegen in
`stages.rs:2030`/`:2056`, im Fixture inaktiv).

**Verbleibende Kandidaten liegen in der Treiber-/wgpu-Schicht.**

### Nebenbefund (ehrlich benannt, NICHT als Ursache behauptet)

Der Input-Upload überspringt auf **Pointer-Identität** statt Content-Hash
(`crates/lumina-gpu/src/lib.rs:3263-3285`) — echtes latentes Risiko, im
konkreten Test nicht auslösbar (gleiche Quelldatei pro Lauf). **Kein Fix.**

## Abgeleitet

- Der rote Lauf des Tests `the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move`
  ist auf dieser Maschine **nicht** reproduzierbar, weil die GPU-Route ohne
  Adapter nicht aktiviert wird. Die CPU-Route ist grün.
- Die ursprünglichen Rot-Läufe (Befund 2026-10-15) waren auf einer Maschine
  mit Adapter aufgetreten; die CPU-Route war dort ebenfalls grün.

## Vermutung (nicht als Befund)

- Readback, uninitialisierter VRAM-Speicher oder ein Race sind **denkbar**.
  Eine Vermutung wird hier **nicht** als Befund ausgegeben.

## Nicht-Behauptungen

- **Keine Aussage**, ob der GPU-Pfad auf Adapter-Hardware wirklich
  nichtdeterministisch ist.
- **Keine Aussage**, der Test sei auf Adapter-Hardware grün.
- **Kein Fix** vorgeschlagen.

## Benannte Grenze

Reproduktion und Behebung des Nichtdeterminismus sind **nur auf einer
Maschine mit gebundenem Adapter** (Metal/Vulkan) möglich. Auf dieser
headless Maschine ist die GPU-Route nicht aktivierbar; die CPU-Route ist
grün und belegt nichts über die GPU-Route.

## Beweisplan (auf Adapter-Hardware)

1. **Reproduktion:** Auf einer Maschine mit gebundem Adapter (Metal/Vulkan)
   den Test `cargo test -p lumina-cli --features gpu --bin lumina-cli tests::auto_tone_float::the_rendered_output_stays_byte_stable_and_the_persisted_text_does_not_move -- --exact`
   fahren. Erwartet: der Test ist laut Ausgangsbefund rot.
2. **Adapter-Nachweis:** `cargo test -p lumina-gpu --features gpu-adapter-tests`
   plus die betroffenen GUI-GPU-Tests (Entscheidung `GPU-E2E`).
3. **Mechanismus eingrenzen:** Den Nichtdeterminismus bis zu einer belegten
   Aussage eingrenzen (z. B. zweimaliger GPU-Render im selben Prozess vs.
   zwei Prozesse, betroffene Stufen, Einfluss der Testparallelität).
4. **Beheben oder Grenze bestätigen:** Den Mechanismus beheben oder als
   dauerhafte Grenze mit Messwerten dokumentieren.

## Abnahme

- Ein **vorher rotes, reproduzierbares** Kommando auf Adapter-Hardware,
  danach grün **oder** eine belegte, benannte Grenze inkl. Adapter-Referenz.
- Verbindliches Kommando: `cargo test -p lumina-gpu --features gpu-adapter-tests`
  plus die betroffenen GUI-GPU-Tests (Regel `GPU-E2E`).
