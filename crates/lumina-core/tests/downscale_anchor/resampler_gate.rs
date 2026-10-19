//! RESAMPLER-GATE-SCOPE-78 — der **Geltungsbereich** des Resampler-Gates.
//!
//! Der Befund F3 des Test-Audits (`TEST-AUDIT-36`, 2026-10-19) war: das Gate
//! `exactly_one_resampler_exists` war eine **3-Datei-String-Blacklist**. Es
//! pruefte nur `lumina-core/src/lib.rs`, `lumina-gui/src/filmstrip.rs` und
//! `lumina-gui/src/preview_jobs.rs` auf die Abwesenheit der drei Altnamen
//! (`downscale_bilinear`, `downscale_rgba`, `downscale_to_target`). Jeder
//! Resampler **ausserhalb** dieser drei Dateien lag damit ausserhalb des
//! Gates — die Masken-Plane-Resampler in `lumina-core/src/render.rs` und
//! `lumina-core/src/mask_alignment.rs` waren nie benannt, und die
//! produktiven Frame-Resampler der ONNX-Adapter (Nearest-Nachbar) wurden
//! gar nicht als solche erkannt.
//!
//! ## Was das Gate **garantiert** (woertlich)
//!
//! > 1. **Kein unklassifizierter Resampler.** Im gesamten Workspace (alle
//! >    Workspace-Crates, je `src/`, `tests/`, `benches/`, `examples/`,
//! >    `bench/`, nur `.rs`; ohne `target/`, `.git/` und ohne die Datei des
//! >    Gates selbst) existiert **keine** Definition, deren **Name** einen
//! >    Stiel aus der geschlossenen Liste traegt **und** deren **Signatur**
//! >    `ImageFrame` nennt, die nicht einer der vier benannten Kategorien
//! >    zugeordnet ist.
//! > 2. **Die Erkennungsmenge ist gleich der erlaubten Menge** — in beide
//! >    Richtungen und je Kategorie geprueft: jeder Kandidat hat genau einen
//! >    Allow-Eintrag, jeder Allow-Eintrag genau einen Kandidaten, und die
//! >    Zaehlung stimmt je Kategorie ueberein. Die Allow-Liste kann daher
//! >    nicht still veralten.
//! > 3. **Der kanonische Downscale-Pfad existiert und funktioniert.**
//! >    `lumina_core::downscale` + `DownscaleTarget` + die Delegation
//! >    `ImageFrame::downscale` muessen im Quelltext stehen; der Test ruft
//! >    den Pfad zusaetzlich auf. Wird der Pfad umbenannt oder entfernt,
//! >    bricht die Kompilierung dieses Tests — das Gate kann nicht still
//! >    gruen bleiben.
//! > 4. **Die drei Altnamen sind workspace-weit nicht mehr definiert**
//! >    (`fn`-Definitionen, exakter Vergleich).
//!
//! ## Was das Gate **ausdruecklich NICHT** garantiert
//!
//! > Beide Erkennungsachsen sind **geschlossen**. Das Gate erfasst
//! > **keinen** Resampler,
//! > * dessen **Name** keinen der Stiele traegt (z. B. `shrink`, `reduce`,
//! >   `fit`, `place`) — die Stiel-Liste ist eine bewusste, benannte
//! >   Entscheidung, keine Vollstaendigkeitszusage; **und/oder**
//! > * dessen **Signatur** kein `ImageFrame` nennt — z. B. eine
//! >   Implementierung, die direkt auf `&[u8]`/`Vec<u8>` rechnet statt auf
//! >   `ImageFrame`; **und/oder**
//! > * der als **Methode** auf einem eigenen Typ (nicht `ImageFrame`)
//! >   definiert ist und ein `ImageFrame` nur ueber einen anderen Weg
//! >   erhaelt.
//! >
//! > Eine neue Achse erfordert eine **bewusste** Aenderung des Gate-Tests.
//!
//! ## Die semantische Suche und die Selbstreferenz
//!
//! Die Begruendung, **warum** die semantische Suche begrenzt ist (Stiel- UND
//! Typ-Anker, gemessene Trefferzahlen, die namentlich benannten
//! Nicht-Kandidaten), steht in `resampler_inventory.rs`; die Erklaerung der
//! Selbstreferenz (der Ausschluss der Gate-Datei als tragender Mechanismus, die
//! Raw-String-Grenze des Detektors) in `resampler_scan.rs`.

use crate::resampler_inventory::{
    category_holds, is_candidate, AllowedDefinition, Category, ALLOWED, PLANE_RESAMPLERS,
};
use crate::resampler_scan::{
    definitions_in_file, squeeze, workspace_root, Definition, Workspace, EXCLUDED_DIRECTORY_NAMES,
    GATE_OWN_FILES,
};
use lumina_core::ImageFrame;

// ---------------------------------------------------------------------------
// 1. Die verbotenen Namen.
// ---------------------------------------------------------------------------

/// Die **verbotenen** Namen: die drei Frame-Resampler, die bei der
/// Konsolidierung (DOWNSCALER-UNIFY-71) entfernt wurden. Sie duerfen im
/// gesamten Workspace nicht mehr **definiert** werden.
const RETIRED_FRAME_RESAMPLER_NAMES: [&str; 3] = [
    "downscale_bilinear",
    "downscale_rgba",
    "downscale_to_target",
];

// ---------------------------------------------------------------------------
// 2. Der kanonische Pfad.
// ---------------------------------------------------------------------------

/// Die Datei, in der der kanonische Pfad stehen **muss**.
const CANONICAL_SOURCE_FILE: &str = "crates/lumina-core/src/lib.rs";

/// Der kanonische Einstieg, die Zielform und die dokumentierte Delegation —
/// jeweils **ohne** Leerraum. Der Vergleich laeuft gegen den
/// Leerraum-entfernten Quelltext: ein reines Umformatieren (Zeilenumbruch,
/// Einrueckung) **oder** ein bloss umbenannter Parameter macht das Gate nicht
/// rot, ein Umbenennen oder Entfernen des kanonischen Pfades aber schon.
/// Gepinnt sind bewusst nur die **Namen** — die *Form* der Signatur wird von
/// der strukturellen Suche geprueft, nicht von Textvergleich.
const CANONICAL_TARGET_ENUM: &str = "pub enum DownscaleTarget";
const CANONICAL_ENTRY_POINT: &str = "pub fn downscale(";
const CANONICAL_DELEGATION: &str = "pub fn downscale(&self,";

/// Die Mindestzahl der gescannten Dateien. **Gemessen am 2026-10-19: 807**
/// (808 `.rs`-Dateien unter den gescannten Unterverzeichnissen, minus die
/// Datei des Gates selbst). Gepinnt mit 7 Dateien Reserve: ein *stiller*
/// Bereichswechsel (weniger Dateien) muss auffallen, ein normales Wachstum
/// nicht.
const MIN_SCANNED_FILES: usize = 800;

// ---------------------------------------------------------------------------
// 3. Das Detektor-Fixture: echte Definitionszeilen (Positiv- und Negativfall).
// ---------------------------------------------------------------------------

/// **Positivfall.** Echte Definitionszeilen — der Detektor muss sie finden.
/// Weil sie in dieser Datei stehen, wuerde der Gate-Test sich selbst treffen,
/// wenn seine Datei mitgescannt wuerde; genau das belegt der Test als
/// Gegenprobe zum Ausschluss (`GATE_OWN_FILES`).
const DETECTOR_FIXTURE: &str = r#"
fn downscale_bilinear(pixels: &[u8], width: u32) -> Vec<u8> {
    pixels.to_vec()
}
pub(crate) fn downscale_rgba(pixels: &[u8]) -> Vec<u8> {
    pixels.to_vec()
}
fn downscale_to_target(pixels: &[u8]) -> (Vec<u8>, u32, u32) {
    (pixels.to_vec(), 0, 0)
}
"#;

/// **Negativfall.** Die Altnamen in einem Kommentar und in einem
/// String-Literal (eine echte Zeile aus `lumina-cull/src/identity.rs`) — das
/// sind **keine** Definitionen und duerfen das Gate nicht ausloesen.
const DETECTOR_NEGATIVE_FIXTURE: &str = r#"
// Der alte MaxWidth-Resampler hiess downscale_bilinear.
pub const HEURISTIC_PREPROCESSING_NAME: &str = "srgb_rgba8_downscale_bilinear";
// pub fn downscale_rgba(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
let embedded = "pub fn downscale_bilinear(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }";
"#;

/// **Sichtbarkeits-Formen.** Der Detektor muss jede als Definition
/// erkennen — insbesondere `pub(in crate::…)`, bei dem der Pfad in Klammern
/// als **eine** Angabe zaehlt (nicht als `pub` + `in` + `crate` + Pfad).
const DETECTOR_VISIBILITY_FIXTURE: &str = r#"
pub fn vis_plain(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub(crate) fn vis_crate(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub(super) fn vis_super(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub(self) fn vis_self(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub(in crate::some_mod) fn vis_in_path(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub(in crate) fn vis_in_bare(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
pub (in crate::some_mod) fn vis_in_spaced(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
const fn vis_const(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
async fn vis_async(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
unsafe fn vis_unsafe(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
extern "C" fn vis_extern(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
default fn vis_default(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
#[inline]
fn vis_attributed(pixels: &[u8]) -> Vec<u8> { pixels.to_vec() }
"#;

/// Die Namen, die das Sichtbarkeits-Fixture definieren muss.
const DETECTOR_VISIBILITY_NAMES: [&str; 13] = [
    "vis_plain",
    "vis_crate",
    "vis_super",
    "vis_self",
    "vis_in_path",
    "vis_in_bare",
    "vis_in_spaced",
    "vis_const",
    "vis_async",
    "vis_unsafe",
    "vis_extern",
    "vis_default",
    "vis_attributed",
];

// ---------------------------------------------------------------------------
// 4. Das Gate.
// ---------------------------------------------------------------------------

#[test]
fn exactly_one_resampler_exists() {
    let root = workspace_root();
    let workspace = Workspace::scan(&root);

    // -- 4.1 Der Suchbereich: Umfang, Anker, Ausschluesse --------------------
    assert!(
        workspace.rel_paths.len() >= MIN_SCANNED_FILES,
        "der Workspace-Scan muss den ganzen Workspace sehen, nicht drei Dateien (gefunden: {} Dateien, gepinnt: mindestens {})",
        workspace.rel_paths.len(),
        MIN_SCANNED_FILES
    );
    for anchor in [
        CANONICAL_SOURCE_FILE,
        "crates/lumina-gui/src/filmstrip.rs",
        "crates/lumina-gui/src/preview_jobs.rs",
        "crates/lumina-cli/src/main.rs",
        "crates/lumina-onnx/src/preprocess.rs",
        "crates/lumina-onnx/src/face/backend.rs",
        "crates/lumina-bench/bench/core.rs",
    ] {
        assert!(
            workspace.rel_paths.iter().any(|path| path == anchor),
            "der Suchbereich muss `{anchor}` umfassen (gefunden: {} Dateien)",
            workspace.rel_paths.len()
        );
    }
    // Der `target/`-`.git/`-Ausschluss ist **belegt**: kein gescannter Pfad
    // enthaelt ein solches Verzeichnis, und die Zahl der weggenommenen
    // Dateien ist gepinnt (heute 0 — unter `crates/` gibt es kein solches
    // Verzeichnis; der Ausschluss ist defensiv, nicht lasttragend).
    for path in &workspace.rel_paths {
        for segment in path.split('/') {
            assert!(
                !EXCLUDED_DIRECTORY_NAMES.contains(&segment),
                "der Ausschluss hat nicht gegriffen: `{path}` enthaelt `{segment}`"
            );
        }
    }
    assert_eq!(
        workspace.excluded_by_directory_rule, 0,
        "unter `crates/` ist ein `target/`- oder `.git/`-Verzeichnis aufgetaucht — der Suchbereich hat sich geaendert und muss bewusst bestaetigt werden"
    );
    for own in GATE_OWN_FILES {
        assert!(
            !workspace.rel_paths.iter().any(|path| path == own),
            "die Gate-Datei `{own}` darf sich nicht selbst scannen"
        );
    }

    // -- 4.2 Der Detektor lebt, und die Selbstreferenz ist ausgeschlossen ----
    let fixture_definitions = definitions_in_file("fixture.rs", DETECTOR_FIXTURE);
    let fixture_hits: Vec<&Definition> = fixture_definitions
        .iter()
        .filter(|definition| RETIRED_FRAME_RESAMPLER_NAMES.contains(&definition.name.as_str()))
        .collect();
    assert_eq!(
        fixture_hits.len(),
        RETIRED_FRAME_RESAMPLER_NAMES.len(),
        "der Detektor muss echte Definitionen finden (gefunden: {:?})",
        fixture_hits
            .iter()
            .map(|definition| definition.location())
            .collect::<Vec<_>>()
    );
    for definition in &fixture_hits {
        assert!(
            definition.line > 1,
            "der Detektor muss die Zeilennummer melden ({})",
            definition.location()
        );
    }
    let negative_definitions = definitions_in_file("negative.rs", DETECTOR_NEGATIVE_FIXTURE);
    let negative_hits: Vec<&Definition> = negative_definitions
        .iter()
        .filter(|definition| RETIRED_FRAME_RESAMPLER_NAMES.contains(&definition.name.as_str()))
        .collect();
    assert!(
        negative_hits.is_empty(),
        "eine blosse Erwaehnung (Kommentar oder String) ist keine Definition (Treffer: {:?})",
        negative_hits
            .iter()
            .map(|definition| definition.location())
            .collect::<Vec<_>>()
    );
    // Die Sichtbarkeits-Formen vor `fn` muessen **alle** als Definition
    // erkannt werden — insbesondere `pub(in crate::…)`, bei dem der Pfad in
    // Klammern als **eine** Angabe zaehlt.
    let visibility_definitions = definitions_in_file("visibility.rs", DETECTOR_VISIBILITY_FIXTURE);
    for name in DETECTOR_VISIBILITY_NAMES {
        assert!(
            visibility_definitions
                .iter()
                .any(|definition| definition.name == name),
            "der Detektor muss die Sichtbarkeits-Form `{}` als Definition erkennen (gefunden: {:?})",
            name,
            visibility_definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>()
        );
    }
    // Gegenprobe zum Ausschluss: **ohne** `GATE_OWN_FILES` wuerde der
    // Gate-Test sich selbst treffen — der Ausschluss ist also der tragende
    // Mechanismus, nicht dekorativ.
    let own_source = std::fs::read_to_string(root.join(GATE_OWN_FILES[0]))
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", GATE_OWN_FILES[0]));
    let own_definitions = definitions_in_file(GATE_OWN_FILES[0], &own_source);
    let own_hits: Vec<&Definition> = own_definitions
        .iter()
        .filter(|definition| RETIRED_FRAME_RESAMPLER_NAMES.contains(&definition.name.as_str()))
        .collect();
    assert!(
        !own_hits.is_empty(),
        "der Selbstbezug muss belegbar sein: ohne den Ausschluss wuerde `{0}` sich selbst treffen",
        GATE_OWN_FILES[0]
    );

    // -- 4.3 Der kanonische Pfad existiert (kein vakuum-gruenes Gate) ---------
    let canonical_source = std::fs::read_to_string(root.join(CANONICAL_SOURCE_FILE))
        .unwrap_or_else(|error| panic!("cannot read {CANONICAL_SOURCE_FILE}: {error}"));
    let canonical_squeezed = squeeze(&canonical_source);
    for expected in [
        CANONICAL_TARGET_ENUM,
        CANONICAL_ENTRY_POINT,
        CANONICAL_DELEGATION,
    ] {
        assert!(
            canonical_squeezed.contains(&squeeze(expected)),
            "der kanonische Pfad muss in {CANONICAL_SOURCE_FILE} stehen: `{expected}` fehlt"
        );
    }
    // Zusaetzlich **verhaltensseitig**: der kanonische Pfad muss aufrufbar
    // sein. Wer ihn umbenennt, bricht damit die Kompilierung dieses Tests —
    // das Gate kann nicht still gruen bleiben.
    let frame = ImageFrame::new(64, 48, vec![7u8; 64 * 48 * 4]).expect("test frame");
    let scaled = lumina_core::downscale(&frame, lumina_core::DownscaleTarget::LongEdge(32))
        .expect("the canonical downscale must work");
    assert_eq!(
        scaled.width().max(scaled.height()),
        32,
        "der kanonische Pfad muss die lange Kante auf 32 begrenzen"
    );

    // -- 4.4 Die drei Altnamen sind workspace-weit nicht mehr definiert ------
    for name in RETIRED_FRAME_RESAMPLER_NAMES {
        let hits: Vec<&Definition> = workspace
            .definitions
            .iter()
            .filter(|definition| definition.name == name)
            .collect();
        assert!(
            hits.is_empty(),
            "der Altname `{name}` darf im Workspace nicht mehr definiert werden — Treffer: {}",
            hits.iter()
                .map(|definition| definition.location())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // -- 4.5 Die Invariante: Erkennungsmenge == erlaubte Menge --------------
    let candidates: Vec<&Definition> = workspace
        .definitions
        .iter()
        .filter(|definition| is_candidate(definition))
        .collect();
    // (a) Jeder Kandidat hat genau EINEN Allow-Eintrag …
    for candidate in &candidates {
        let matches: Vec<&AllowedDefinition> = ALLOWED
            .iter()
            .filter(|entry| {
                entry.path == candidate.path
                    && entry.impl_type == candidate.impl_type
                    && entry.name == candidate.name
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "{} `fn {}` hat {} Allow-Eintraege (erwartet: genau 1) — {}",
            candidate.location(),
            candidate.name,
            matches.len(),
            if matches.is_empty() {
                "der Name traegt einen Resampler-Stiel und die Signatur nennt `ImageFrame`, aber es gibt **keine** benannte Ausnahme"
            } else {
                "mehrere Allow-Eintraege fuer dieselbe Definition sind nicht zulaessig — die Allow-Liste muss eindeutig sein"
            }
        );
        let entry = matches[0];
        assert!(
            category_holds(candidate, entry.category),
            "{} `fn {}` erfuellt die Formregel der Kategorie `{}` nicht mehr — die Ausnahme ist veraltet oder falsch zugeordnet (Begruendung: {})",
            candidate.location(),
            candidate.name,
            entry.category.label(),
            entry.reason
        );
    }
    // (b) … und jeder Allow-Eintrag hat genau EINEN Kandidaten.
    for entry in &ALLOWED {
        let matches: Vec<&&Definition> = candidates
            .iter()
            .filter(|candidate| {
                candidate.path == entry.path
                    && candidate.impl_type == entry.impl_type
                    && candidate.name == entry.name
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "die Ausnahme `{}` ({}) existiert nicht mehr (oder ist doppelt) — eine Umbenennung muss eine bewusste Anpassung erzwingen (bisherige Begruendung: {})",
            entry.name,
            entry.path,
            entry.reason
        );
    }
    // (c) Je Kategorie stimmen die Zaehlungen ueberein.
    for category in Category::all() {
        let allowed = ALLOWED
            .iter()
            .filter(|entry| entry.category == category)
            .count();
        let detected = candidates
            .iter()
            .filter(|candidate| {
                ALLOWED.iter().any(|entry| {
                    entry.category == category
                        && entry.path == candidate.path
                        && entry.impl_type == candidate.impl_type
                        && entry.name == candidate.name
                })
            })
            .count();
        assert_eq!(
            allowed,
            detected,
            "die Erkennungsmenge der Kategorie `{}` weicht von der erlaubten Menge ab",
            category.label()
        );
    }
    // (d) Der kanonische Downscale bleibt **einzig**.
    let canonical = candidates
        .iter()
        .filter(|candidate| {
            ALLOWED.iter().any(|entry| {
                entry.category == Category::CanonicalDownscale
                    && entry.path == candidate.path
                    && entry.impl_type == candidate.impl_type
                    && entry.name == candidate.name
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        canonical.len(),
        ALLOWED
            .iter()
            .filter(|entry| entry.category == Category::CanonicalDownscale)
            .count(),
        "es muss genau EINEN kanonischen Downscale (freie Funktion + Delegation) geben"
    );

    // -- 4.6 Die Frame-vs-Masken-Plane-Grenze (Dokumentationsliste) ---------
    // Die Liste ist **bewusst nicht exhaustiv**: sie benennt die produktiven
    // Ebenen-Resampler (Masken-Plane, Luma-Plane). Die **nicht** erfassten
    // Familien — Punktabtastung (`sample_bilinear`), Warp ohne Groessenaenderung
    // (`cylindrical_warp_plane`), WGSL-Shader-Quelltext (`bilinear255`) — sind
    // in `resampler_inventory.rs` namentlich mit Grund aufgefuehrt.
    for entry in &PLANE_RESAMPLERS {
        let definition = workspace
            .definitions
            .iter()
            .find(|definition| {
                definition.path == entry.path
                    && definition.impl_type == entry.impl_type
                    && definition.name == entry.name
            })
            .unwrap_or_else(|| {
                panic!(
                    "die Plane-Grenze nennt `{}` ({}), aber dort gibt es keine solche Definition mehr — eine Umbenennung muss eine bewusste Anpassung erzwingen (bisherige Begruendung: {})",
                    entry.name, entry.path, entry.reason
                )
            });
        assert!(
            !definition.params.contains("ImageFrame") && !definition.returns.contains("ImageFrame"),
            "`{}` ({}) erzeugt Frame-Bytes — das ist kein Plane-Resampler, sondern ein Frame-Resampler und gehoert in eine der vier Kategorien (Begruendung der Liste: {})",
            entry.name,
            entry.path,
            entry.reason
        );
    }
}
