# Backend-Entscheidungen und Lizenzbedingungen

**Features:** F-007 RAW-Backend, ONNX-Backend, GUI-Framework, F-006, F-010

Dieses Dokument fasst die getroffenen Backend-Entscheidungen und ihre
Lizenzbedingungen zusammen. Architektonische Begründungen sind zusätzlich als
ADRs unter `docs/adr/` festgehalten (siehe `docs/adr/README.md`).

## RAW-Backend: native LibRaw

- **Entscheidung:** RAW-Decodierung erfolgt im MVP über einen gekapselten
  nativen LibRaw-Adapter (`crates/lumina-raw` über `libraw-sys`).
- **Lizenz (LibRaw):** LibRaw wird **dual-lizenziert** unter
  **GNU Lesser General Public License v2.1 (LGPL-2.1)** *und*
  **Common Development and Distribution License v1.0 (CDDL-1.0)**. Anwender
  können die für ihren Fall passende Lizenz wählen. (Quelle: libraw.org/about.)
- **Lizenz (Rust-Bindings):** Das verwendete `libraw-sys`-Crate (FFI-Bindings,
  im Workspace unter `vendor/libraw-sys` gepatcht) steht unter **MIT**. Die
  MIT-Lizenz des Bindings entbindet nicht von den Bedingungen der darunter
  linkten LibRaw-C++-Bibliothek.
- **Lizenzpflichten:** Bei Distribution von Lumina (nativ) ist die gewählte
  LibRaw-Lizenz (LGPL-2.1 oder CDDL-1.0) samt Hinweisen einzuhalten; bei LGPL
  sind die LibRaw-Quellen bzw. ein schriftliches Angebot bereitzustellen. Dies
  ist vor dem ersten Release (F-078) final zu prüfen.

## GUI-Framework: egui / eframe

- **Entscheidung:** Die Desktop-GUI verwendet **egui/eframe** (v1). Tauri ist
  keine v1-Abhängigkeit und kann in einer späteren Architekturentscheidung
  erneut bewertet werden.
- `lumina-gui` ist bewusst sowohl nativ (eframe) als auch als Trunk-WASM-App
  ausgelegt; native Abhängigkeiten (`rfd`) und WASM-Abhängigkeiten
  (`wasm-bindgen`, `web-sys`) sind über `cfg(target_arch = "wasm32")`
  getrennt.

## ONNX-Backend: native Inferenz, post-MVP

- **Entscheidung:** KI-Masken (BiRefNet als erstes automatisches Subject-Modell,
  SAM 2 als erstes interaktives Box-/Pinsel-Modell) werden über einen
  **austauschbaren ONNX-Adapter** (`lumina-onnx`, post-MVP) angebunden.
- Der ONNX-Adapter muss native Inferenz, Modellverwaltung und Maskenartefakte
  kapseln und darf den plattformneutralen `lumina-core` nicht belasten.
- **Lizenz:** Die ONNX-Runtime und die Modelle (BiRefNet, SAM 2) sind vor
  Integration nach ihren jeweiligen Lizenzen zu prüfen und zu dokumentieren
  (F-078). Die Modellfähigkeit wird aus dem Modellmanifest gelesen, nicht aus
  dem Modellnamen erraten.

## Arbeitsweise

- Lizenzbedingungen von RAW-Backends, ONNX-Runtime und Modellen werden vor
  Integration geprüft und dokumentiert (Agents.md, Änderungsregeln).
- Abweichungen von diesen Entscheidungen werden als neue ADR dokumentiert.
