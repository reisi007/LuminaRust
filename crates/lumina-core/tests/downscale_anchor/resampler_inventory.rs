//! RESAMPLER-GATE-SCOPE-78 — die **Kategorien** des Resampler-Gates.
//!
//! Das Gate kennt zwei Ebenen:
//!
//! 1. **Kandidaten** (mechanisch erkannt): Definitionen, deren **Name** einen
//!    Stiel aus [`RESAMPLER_NAME_STEMS`] traegt **und** deren **Signatur**
//!    `ImageFrame` nennt. Beide Achsen sind **geschlossen**.
//! 2. **Kategorien** (bewusst gepflegt): jeder Kandidat muss genau einer
//!    benannten Kategorie zugeordnet sein — mit Begruendung, am **Namen**
//!    gepinnt (Pfad + `impl`-Typ + Funktionsname), **ohne** Zeilennummern.
//!
//! Die vier Kategorien:
//!
//! * [`Category::CanonicalDownscale`] — der kanonische Downscale-Pfad
//!   (`lumina_core::downscale` + `DownscaleTarget`) und seine Delegation.
//!   Das ist der **einzige** erlaubte Produkt-Downscale.
//! * [`Category::FrameToPlane`] — verbraucht Frame-Pixel, erzeugt aber eine
//!   **Ebene** (Luma-Ebene), keine Frame-Bytes.
//! * [`Category::ModelAdapter`] — die **Modell-Adapter-Grenze**: frame ->
//!   Modellaufloesung mit eigener nearest-Vorschrift. Kein Produkt-Downscale.
//! * [`Category::TestAssertion`] — Test-Assertions, die Frame-Pixel
//!   *pruefen*, aber keine erzeugen.
//!
//! Zusaetzlich gibt es die **Dokumentationsliste** der Plane-Resampler
//! ([`PLANE_RESAMPLERS`]): sie benennt die Masken-/Luma-Ebenen-Resampler, die
//! bewusst zugelassen sind. Sie ist **bewusst nicht exhaustiv** — siehe die
//! Modul-Doku von `resampler_gate.rs`.
//! **Namentlich bekannte Nicht-Kandidaten** (im Inventar, aber ausserhalb der
//! Erkennungsmenge, weil sie an der Namens- oder der Typ-Achse scheitern):
//!
//! * `f32_long_edge_replication`
//!   (`crates/lumina-core/tests/downscale_anchor_support/mod.rs`) — die
//!   **testseitige** Replikation des historischen f32-Kernels (HEAD
//!   `cb2da59`), mit der die Byte-Gleichheit des Byte-Ankers belegt wird. Der
//!   Name traegt keinen Resampler-Stiel; es ist **kein** Produktpfad.
//! * `nearest_replication` (dieselbe Datei) — die testseitige Replikation des
//!   geloeschten Nearest-Nachbar-Thumbnail-Resamplers, mit der der
//!   Nearest->Bilinear-Qualitaetsvergleich gepinnt ist. Ebenfalls **kein**
//!   Produktpfad.
//!
//! ## Die semantische Suche: bewusst **begrenzt** gebaut
//!
//! Ein offener „finde jeden Interpolierer"-Scan erzeugt Falsch-Positive. Am
//! HEAD gemessen (2026-10-19, `crates/*/{src,tests,benches,examples,bench}/**/*.rs`):
//!
//! * **unbegrenzt** (nur der Namens-Stiel, ohne Typ-Anker): **66** Treffer
//!   (die 9-Stiel-Menge allein traf **37**; neu kommen die Familien `align*`
//!   — Merge-/HDR-/Pano-Ausrichtung, ROI-Ausrichtung, Sidecar-Alignment — und
//!   `preprocess*` — cull-Heuristik-Preprocessing, ONNX-Modell-Preprocessing).
//! * **begrenzt** (Stiel **und** `ImageFrame` in der Signatur): **8**
//!   Treffer — genau die acht erlaubten Definitionen.
//!
//! Der Typ-Anker filtert also **58** der 66 Stiel-Treffer weg; die 8
//! ueberlebenden sind **alle** klassifiziert. Details und Begruendungen in
//! `resampler_inventory.rs`.
//!

use crate::resampler_scan::Definition;

/// Die Namensstiele, die eine Definition als "resampler-geformt" kennzeichnen.
///
/// **Geschlossen.** Bewusst enthalten sind auch die beiden Verben, die im
/// Workspace die *produktiven* Frame-Resampler ohne klassischen Stiel tragen:
/// `align` (`align_face_to_template`) und `preprocess`
/// (`preprocess_rgb_to_model`, `preprocess_image_to_model`).
///
/// **Gemessen am 2026-10-19** (`crates/*/{src,tests,benches,examples,bench}/**/*.rs`):
///
/// * **unbegrenzt** (nur dieser Stiel, ohne Typ-Anker): **66** Definitionen —
///   davon 29 neu gegenueber der alten 9-Stiel-Menge (37 Treffer). Die neuen
///   Familien sind: `align*` (Merge-/HDR-/Pano-Ausrichtung, ROI-Ausrichtung,
///   Sidecar-Alignment-Validierung, `aligned_bytes_per_row`) und `preprocess*`
///   (cull-Heuristik-Preprocessing, ONNX-Modell-Preprocessing).
/// * **begrenzt** (Stiel **und** `ImageFrame` in der Signatur): **8**
///   Definitionen — genau die acht in [`ALLOWED`].
///
/// Der Typ-Anker filtert also **58** der 66 Stiel-Treffer weg. Die
/// ueberlebenden acht sind alle klassifiziert; **kein** unklassifizierter
/// Treffer.
pub const RESAMPLER_NAME_STEMS: [&str; 11] = [
    "downscale",
    "resample",
    "resize",
    "downsample",
    "interpolat",
    "bilinear",
    "lanczos",
    "bicubic",
    "nearest",
    "align",
    "preprocess",
];

/// Die Kategorie, in die eine erkannte Definition faellt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Der kanonische Downscale-Pfad und seine Delegation.
    CanonicalDownscale,
    /// Frame-Pixel -> Ebene (keine Frame-Bytes).
    FrameToPlane,
    /// Modell-Adapter-Grenze: frame -> Modellaufloesung, eigene nearest-Vorschrift.
    ModelAdapter,
    /// Test-Assertion: prueft Frame-Pixel, erzeugt keine.
    TestAssertion,
}

impl Category {
    /// Der maschinenlesbare Name der Kategorie (fuer Fehlertexte).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Category::CanonicalDownscale => "kanonischer Downscale",
            Category::FrameToPlane => "Frame->Plane",
            Category::ModelAdapter => "Modell-Adapter",
            Category::TestAssertion => "Test-Assertion",
        }
    }

    /// Alle Kategorien (fuer die Invariante "Erkennungsmenge == erlaubte Menge").
    #[must_use]
    pub fn all() -> [Category; 4] {
        [
            Category::CanonicalDownscale,
            Category::FrameToPlane,
            Category::ModelAdapter,
            Category::TestAssertion,
        ]
    }
}

/// Eine erlaubte Definition. Gepinnt werden **Pfad**, umgebender `impl`-Typ
/// und **Name** — bewusst **keine Zeilennummer**: eine Umbenennung oder ein
/// Umzug erzwingt so eine bewusste Anpassung dieser Liste, statt still zu
/// verschwinden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedDefinition {
    /// Pfad relativ zum Workspace-Root.
    pub path: &'static str,
    /// Der umgebende `impl`-Typ; `""` = frei stehende Funktion.
    pub impl_type: &'static str,
    /// Der Name der Definition.
    pub name: &'static str,
    /// Die Kategorie, in der die Definition liegt.
    pub category: Category,
    /// Begruendung, warum das kein zweiter Produkt-Downscale ist.
    pub reason: &'static str,
}

/// Die **erlaubten** Definitionen — je Kategorie, mit Begruendung.
///
/// Die Liste ist **lueckenlos** gegenueber der Erkennungsmenge: der Test
/// prueft die Invariante in **beide** Richtungen (jeder Kandidat hat genau
/// einen Eintrag, jeder Eintrag genau einen Kandidaten) und je Kategorie.
pub const ALLOWED: [AllowedDefinition; 8] = [
    // -- Kategorie 1: der kanonische Downscale (Produkt) --------------------
    AllowedDefinition {
        path: "crates/lumina-core/src/lib.rs",
        impl_type: "",
        name: "downscale",
        category: Category::CanonicalDownscale,
        reason: "der eine kanonische Downscale-Pfad (`lumina_core::downscale` mit `DownscaleTarget`)",
    },
    AllowedDefinition {
        path: "crates/lumina-core/src/lib.rs",
        impl_type: "ImageFrame",
        name: "downscale",
        category: Category::CanonicalDownscale,
        reason: "die dokumentierte Delegation `ImageFrame::downscale` -> `downscale(..., LongEdge)`",
    },
    // -- Kategorie 2: Frame -> Ebene (keine Frame-Bytes) --------------------
    AllowedDefinition {
        path: "crates/lumina-core/src/upright.rs",
        impl_type: "",
        name: "luma_downsample",
        category: Category::FrameToPlane,
        reason: "Frame -> Luma-Ebene (Rec.709, Kastenmittel) fuer die Aufrechtigkeits-Analyse — erzeugt keine Frame-Bytes",
    },
    // -- Kategorie 3: Modell-Adapter-Grenze ---------------------------------
    AllowedDefinition {
        path: "crates/lumina-onnx/src/face/backend.rs",
        impl_type: "",
        name: "align_face_to_template",
        category: Category::ModelAdapter,
        reason: "Modell-Adapter: Similarity-Warp des Frames auf die 5-Punkt-Template-Aufloesung (nearest, eigene Vorschrift) — kein Produkt-Downscale",
    },
    AllowedDefinition {
        path: "crates/lumina-onnx/src/preprocess.rs",
        impl_type: "",
        name: "preprocess_rgb_to_model",
        category: Category::ModelAdapter,
        reason: "Modell-Adapter: nearest-Resize der RGB-Kanaele auf die Modellaufloesung (integer-Mapping) — die dokumentierte Adapter-Grenze, kein Produkt-Downscale",
    },
    AllowedDefinition {
        path: "crates/lumina-onnx/src/preprocess.rs",
        impl_type: "",
        name: "preprocess_image_to_model",
        category: Category::ModelAdapter,
        reason: "Modell-Adapter: dasselbe nearest-Resize plus Kanal-Layout (RGB/BGR) — die dokumentierte Adapter-Grenze, kein Produkt-Downscale",
    },
    // -- Kategorie 4: Test-Assertions ---------------------------------------
    AllowedDefinition {
        path: "crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs",
        impl_type: "",
        name: "assert_thumbnail_is_really_resampled",
        category: Category::TestAssertion,
        reason: "Test-Assertion: prueft, dass das Thumbnail wirklich neu abgetastet wurde — erzeugt keine Pixel",
    },
    AllowedDefinition {
        path: "crates/lumina-gui/src/tests/histogram/preview_thumb_parity.rs",
        impl_type: "",
        name: "assert_ramp_thumbnail_is_bilinear",
        category: Category::TestAssertion,
        reason: "Test-Assertion: prueft die Bilinear-Eigenschaft des Thumbnails — erzeugt keine Pixel",
    },
];

/// Eine bewusst zugelassene **Plane**-Resampler-Definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneResampler {
    /// Pfad relativ zum Workspace-Root.
    pub path: &'static str,
    /// Der umgebende `impl`-Typ; `""` = frei stehende Funktion.
    pub impl_type: &'static str,
    /// Der Name der Definition.
    pub name: &'static str,
    /// Begruendung, warum das kein Frame-Resampler ist.
    pub reason: &'static str,
}

/// Die **Dokumentationsliste** der Plane-Resampler.
///
/// Diese Liste ist **bewusst nicht exhaustiv** — sie benennt die
/// Frame-vs-Masken-Plane-Grenze an den *produktiven* Staellen, nicht alle
/// ebenen-Resampler im Workspace. **Nicht** erfasst (bewusst, mit Grund):
///
/// * `sample_bilinear` (`lumina-core/src/merge_geom.rs`) — eine
///   **Punktabtastung** eines f32-Ebenen-Puffers, kein Resampler (keine
///   Groessenaenderung).
/// * `cylindrical_warp_plane` (`lumina-core/src/merge_geom.rs`) — ein Warp mit
///   **gleicher** Ausgabe- und Eingabe-Groesse, also keine Groessenaenderung.
/// * `bilinear255` (`lumina-gpu/src/geometry_shaders.rs`,
///   `lumina-gpu/src/lensfun.rs`) — **WGSL-Shader-Quelltext**, kein Rust-
///   Frame-Pfad.
///
/// Diese drei Familien nennen `ImageFrame` nicht und erzeugen keine
/// Frame-Bytes; sie sind daher **keine Kandidaten** des Gates.
pub const PLANE_RESAMPLERS: [PlaneResampler; 4] = [
    PlaneResampler {
        path: "crates/lumina-core/src/render.rs",
        impl_type: "",
        name: "resample_plane_bilinear",
        reason: "resampelt die Masken-Plane (u16) auf die Frame-Groesse — resampelt die Maske, nicht das Frame",
    },
    PlaneResampler {
        path: "crates/lumina-core/src/mask_alignment.rs",
        impl_type: "",
        name: "resample_plane_bilinear",
        reason: "resampelt die Masken-Plane (u16) fuer die Output-Ausrichtung — resampelt die Maske, nicht das Frame",
    },
    PlaneResampler {
        path: "crates/lumina-onnx/src/preprocess.rs",
        impl_type: "",
        name: "rescale_model_matte",
        reason: "resampelt die ONNX-Matte (MaskPlane) zurueck auf die Quellgroesse — resampelt die Maske, nicht das Frame (dieselbe Datei resampelt ueber `preprocess_rgb_to_model` das Frame — die Regel greift je *Definition*, nicht je Datei)",
    },
    PlaneResampler {
        path: "crates/lumina-cull/src/similarity.rs",
        impl_type: "",
        name: "resize_box",
        reason: "Box-Average einer Luma-Plane (Graustufen) — erzeugt keinen Frame",
    },
];

/// Die Typnamen, die eine **Ebene** (kein Frame) kennzeichnen.
pub const PLANE_TYPE_NAMES: [&str; 3] = ["MaskPlane", "LumaPlane", "Vec<f32>"];

/// Ist die Definition ein **Kandidat**? (Stiel **und** `ImageFrame` in der Signatur)
#[must_use]
pub fn is_candidate(definition: &Definition) -> bool {
    let lowercase = definition.name.to_lowercase();
    let stemmed = RESAMPLER_NAME_STEMS
        .iter()
        .any(|stem| lowercase.contains(stem));
    stemmed && definition.touches_frame()
}

/// Erfuellt die Definition die **Formregel** ihrer Kategorie? Das ist die
/// maschinelle Haelfte der Begruendung: sie prueft, dass die Definition
/// wirklich das ist, wofuer sie eingetragen ist.
#[must_use]
pub fn category_holds(definition: &Definition, category: Category) -> bool {
    let params = &definition.params;
    // Der Rueckgabetyp ohne den `->`-Pfeil.
    let returns = definition.returns.trim().trim_start_matches("->").trim();
    match category {
        // Der kanonische Pfad nimmt die Zielform `DownscaleTarget`; die
        // Delegation kann nicht scheitern (Rueckgabetyp exakt `ImageFrame`).
        Category::CanonicalDownscale => {
            params.contains("DownscaleTarget") || returns == "ImageFrame"
        }
        // Frame -> Ebene: keine Frame-Bytes im Rueckgabetyp, aber eine Ebene.
        Category::FrameToPlane => {
            !returns.contains("ImageFrame")
                && (PLANE_TYPE_NAMES
                    .iter()
                    .any(|plane| returns.contains(plane) || params.contains(plane)))
        }
        // Modell-Adapter: verbraucht das Frame und nennt eine Ziel-Aufloesung.
        Category::ModelAdapter => {
            (params.contains("ImageFrame") || returns.contains("ImageFrame"))
                && (params.contains("Resolution") || params.contains("(u32, u32)"))
        }
        // Test-Assertion: nimmt Frame-Pixel, gibt nichts zurueck.
        Category::TestAssertion => returns.is_empty() && params.contains("ImageFrame"),
    }
}
