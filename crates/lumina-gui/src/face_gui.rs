//! LRPAR-G12-FACE-20 (FACE-20-S5) — GUI slice: Library People view.
//!
//! SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §3 (Personen-Ansicht,
//! confirm/split/merge, explicit naming, person filter), §4 (Sidecar-first,
//! source-level clusters/persons) and §5 (**Karten-/GPS-Modul ist nie Ziel** —
//! no geotags are read, persisted or displayed anywhere in this module).
//!
//! ## Pure data operations
//!
//! Confirm/Split/Merge are the pure `lumina-onnx` operations
//! ([`lumina_onnx::confirm_person`], [`lumina_onnx::split_cluster`],
//! [`lumina_onnx::merge_clusters`]) applied to the source-level
//! `SidecarDocument::face` section. There is no GUI-side face algorithm and no
//! second clustering path. The mutated section is validated with
//! `validate_face_analysis` and written through the shared compare-and-swap
//! atomic sidecar writer; `rating`/`flag`/`color_label`/recipe are untouched.
//!
//! ## Names are explicit user data
//!
//! A name is only ever assigned by the explicit Confirm action (never derived
//! from detection scores or any automatic label). The person id is derived
//! deterministically from the name so re-confirming the same name targets the
//! same person; nothing is uploaded or compared across images (image-local
//! clusters only, §2.3/§4).
//!
//! ## Status warnings (real evidence, never presence-only)
//!
//! [`FaceViewStatus`] surfaces `no analysis` / `valid` / `stale` / `missing` /
//! `corrupt`. `stale`/`missing`/`corrupt` are classified by the shared
//! [`lumina_onnx::face_artifact_status`] against the live source/decode/
//! geometry context and **real evidence** from the referenced embedding
//! artefacts: every referenced `face_embedding` record payload is read and
//! hashed (BLAKE3), and the digest is compared with the persisted record
//! checksum. A record that exists but
//! does not hash to the persisted checksum is `corrupt` (never a false
//! `valid`); a missing file is `missing`; an analysis whose vectors were never
//! persisted carries no verifiable payload and is reported `missing` rather
//! than claiming `valid` from an empty reference set. A stale analysis is
//! never presented as current and never re-run silently.
//!
//! ## Develop bridge (S5)
//!
//! [`LuminaApp::create_face_mask`] is the FACE-20 §3 Develop bridge: it turns
//! one persisted, `valid` face detection box into a deterministic mask source
//! on the active virtual copy ([`MaskPrompt::Box`], geometrically rasterized by
//! the shared `lumina-core` mask graph — the same path every other prompt mask
//! uses). It never invents a region: an absent (`no analysis`), `stale`,
//! `missing` or `corrupt` analysis is a loud refusal, and the persisted
//! detection id + face identity digest are stored as the prompt provenance so
//! the source stays reproducible. The generic [`lumina_sidecar::AiSelectKind::People`]
//! AI-selector is a *separate* model-dependent path (it needs a loaded or
//! inferred plane); it is not silently rewired to the detection boxes.

use log::info;
use lumina_onnx::{
    confirm_person, face_artifact_status, face_identity_digest, merge_clusters, split_cluster,
    FaceArtifactEvidence,
};
use lumina_sidecar::{
    save_sidecar_if_unchanged, sidecar_path_for, validate_face_analysis, FaceAnalysis,
    FaceArtifactStatus, FaceBoundingBox, FaceCluster, MaskPrompt, MaskStatus, NormalizedRect,
    PromptTransform,
};
use std::path::Path;

use crate::i18n::Str;
use crate::{GuiError, LuminaApp};

/// Visible face-analysis state of the active image (FACE-20 §4 table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceViewStatus {
    /// No `face` section: the valid "no analysis" state.
    NoAnalysis,
    Valid,
    Stale,
    Missing,
    Corrupt,
}

impl FaceViewStatus {
    fn from_artifact(status: FaceArtifactStatus) -> Self {
        match status {
            FaceArtifactStatus::Valid => FaceViewStatus::Valid,
            FaceArtifactStatus::Stale => FaceViewStatus::Stale,
            FaceArtifactStatus::Missing => FaceViewStatus::Missing,
            FaceArtifactStatus::Corrupt => FaceViewStatus::Corrupt,
        }
    }

    /// Visible status text.
    fn text(self) -> &'static str {
        match self {
            FaceViewStatus::NoAnalysis => Str::FaceNoAnalysis.t(),
            FaceViewStatus::Valid => Str::FaceStatusValid.t(),
            FaceViewStatus::Stale => Str::FaceStatusStale.t(),
            FaceViewStatus::Missing => Str::FaceStatusMissing.t(),
            FaceViewStatus::Corrupt => Str::FaceStatusCorrupt.t(),
        }
    }
}

/// Deterministic person id for a user-entered name (stable across sessions;
/// image-local, never a cross-catalogue identity — FACE-20 §4).
pub fn face_person_id_for(name: &str) -> String {
    let digest = blake3::hash(name.trim().as_bytes()).to_hex().to_string();
    format!("person-{}", &digest[..32])
}

/// Deterministic id of the cluster produced by a Split (content-derived from
/// the moved members, never an array position).
pub fn face_split_cluster_id(subset: &[String]) -> String {
    let mut members: Vec<&str> = subset.iter().map(String::as_str).collect();
    members.sort_unstable();
    let digest = blake3::hash(members.join("|").as_bytes())
        .to_hex()
        .to_string();
    format!("cluster-{}", &digest[..32])
}

impl LuminaApp {
    /// The active image's face section, if any.
    pub fn face_analysis(&self) -> Option<&FaceAnalysis> {
        self.document.as_ref()?.face.as_ref()
    }

    /// Classifies the active image's face analysis against the live context
    /// (FACE-20 §4). Pure read; never re-runs inference.
    pub fn face_view_status(&mut self) -> FaceViewStatus {
        let Some(analysis) = self.face_analysis().cloned() else {
            return FaceViewStatus::NoAnalysis;
        };
        if analysis.status != FaceArtifactStatus::Valid {
            return FaceViewStatus::from_artifact(analysis.status);
        }
        let Some(frame) = self.original.as_ref().cloned() else {
            return FaceViewStatus::NoAnalysis;
        };
        let source_hash = self.resolved_source_hash();
        let Some(document) = self.document.as_ref() else {
            return FaceViewStatus::NoAnalysis;
        };
        let byte_length = document.source.byte_length;
        let decode = document.source.decode_fingerprint.clone();
        let mut current = analysis.identity.clone();
        current.source.content_hash = source_hash;
        current.source.byte_length = byte_length;
        current.decode = decode;
        current.geometry.width = frame.width;
        current.geometry.height = frame.height;
        current.geometry.orientation = self.raw_orientation;
        let bundle_root = Path::new(&self.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let evidence = face_artifact_evidence(&bundle_root, &analysis);
        FaceViewStatus::from_artifact(face_artifact_status(&current, &analysis.identity, evidence))
    }

    /// FACE-20 §3 Develop bridge: create a deterministic mask source on the
    /// active virtual copy from one persisted face detection box.
    ///
    /// The matte is the detection's normalized bounding box
    /// ([`MaskPrompt::Box`]), rasterized geometrically by the shared
    /// `lumina-core` mask graph — the same, model-free path every other prompt
    /// source uses. The detection id and the analysis identity digest are
    /// stored as prompt provenance, so the source stays reproducible and is
    /// tied to the exact persisted analysis that produced it.
    ///
    /// Loud, never a silent fallback: an absent analysis (`no analysis`) or one
    /// that is `stale`/`missing`/`corrupt` is refused, an unknown detection id
    /// is refused, and a mask with the derived id already on the copy is
    /// refused (the id is a pure function of the identity digest + detection,
    /// so re-running the same bridge cannot silently duplicate or overwrite).
    pub fn create_face_mask(
        &mut self,
        detection_id: &str,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        match self.face_view_status() {
            FaceViewStatus::Valid => {}
            status => {
                return Err(GuiError::Io(
                    Str::FaceMaskNeedsValidPattern.format_arg(status.text()),
                ))
            }
        }
        let analysis = self.current_face_analysis()?;
        let detection = analysis
            .detections
            .iter()
            .find(|detection| detection.id == detection_id)
            .ok_or_else(|| GuiError::Io(Str::FaceDetectionNotFound.t().to_string()))?;
        let identity_digest = face_identity_digest(&analysis.identity);
        let id = format!(
            "mask-{}",
            blake3::hash(format!("face\0{identity_digest}\0{detection_id}").as_bytes()).to_hex()
        );
        let mut definition = self.new_source_mask_template(&id, &name, MaskStatus::Valid)?;
        definition.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: detection.bbox.x,
                y: detection.bbox.y,
                width: detection.bbox.width,
                height: detection.bbox.height,
            },
            transformation: PromptTransform {
                method: "face-detection".into(),
                parameters: std::collections::BTreeMap::from([
                    ("detection_id".to_string(), detection.id.clone()),
                    ("face_identity_digest".to_string(), identity_digest),
                ]),
            },
        });
        let id = self.push_mask_definition(definition)?;
        self.select_mask(&id)?;
        info!("GUI interaction: create_face_mask {detection_id} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// Explicit name assignment (Confirm): creates or appends to the person
    /// with the deterministic id of `name` (never an automatic label).
    pub fn face_confirm_person(&mut self, cluster_id: &str, name: &str) -> Result<(), GuiError> {
        let analysis = self.current_face_analysis()?;
        let person_id = analysis
            .persons
            .iter()
            .find(|person| person.name == name)
            .map(|person| person.id.clone())
            .unwrap_or_else(|| face_person_id_for(name));
        let (clusters, persons) = confirm_person(
            &analysis.clusters,
            &analysis.persons,
            cluster_id,
            &person_id,
            name,
        )
        .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut updated = analysis;
        updated.clusters = clusters;
        updated.persons = persons;
        self.persist_face_analysis(updated, "face confirm")
    }

    /// Split `subset` (detection ids, comma separated by the panel) out of
    /// `cluster_id` into a new content-derived cluster.
    pub fn face_split_cluster(
        &mut self,
        cluster_id: &str,
        subset: &[String],
    ) -> Result<(), GuiError> {
        let analysis = self.current_face_analysis()?;
        let new_id = face_split_cluster_id(subset);
        let clusters = split_cluster(&analysis.clusters, cluster_id, subset, &new_id)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut updated = analysis;
        updated.clusters = clusters;
        self.persist_face_analysis(updated, "face split")
    }

    /// Merge two clusters; the merged cluster keeps `first_id`.
    pub fn face_merge_clusters(&mut self, first_id: &str, second_id: &str) -> Result<(), GuiError> {
        let analysis = self.current_face_analysis()?;
        let clusters = merge_clusters(&analysis.clusters, first_id, second_id, first_id)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        let mut updated = analysis;
        updated.clusters = clusters;
        self.persist_face_analysis(updated, "face merge")
    }

    fn current_face_analysis(&self) -> Result<FaceAnalysis, GuiError> {
        self.face_analysis()
            .cloned()
            .ok_or_else(|| GuiError::Io(Str::FaceNoAnalysis.t().to_string()))
    }

    /// Validates and persists the mutated face section through the shared CAS
    /// sidecar writer (source-level only; recipe/copies untouched).
    fn persist_face_analysis(
        &mut self,
        analysis: FaceAnalysis,
        action: &str,
    ) -> Result<(), GuiError> {
        validate_face_analysis(&analysis).map_err(|error| GuiError::Io(error.to_string()))?;
        self.ensure_document_loaded()?;
        let path = self.path.clone();
        let mut document = self
            .document
            .take()
            .ok_or_else(|| GuiError::Io(Str::FaceNoAnalysis.t().to_string()))?;
        document.face = Some(analysis);
        let expected = self.sidecar_revision.clone();
        let sidecar = sidecar_path_for(std::path::Path::new(&path));
        match save_sidecar_if_unchanged(&sidecar, &document, expected.as_deref()) {
            Ok(revision) => {
                self.sidecar_revision = Some(revision);
                self.document = Some(document);
                self.refresh_entry(std::path::Path::new(&path));
                info!("{action} persisted for `{path}` (face section only)");
                Ok(())
            }
            Err(error) => {
                self.document = Some(document);
                Err(GuiError::Sidecar(error))
            }
        }
    }

    /// Person names of the active image (grid `person:` filter data).
    pub fn face_person_names(&self) -> Vec<String> {
        self.face_analysis()
            .map(|analysis| {
                analysis
                    .persons
                    .iter()
                    .map(|person| person.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Library People view (FACE-20 §3): status, cluster/person list with the
    /// explicit name/split/merge actions and the face-crop strip of the
    /// selected cluster. Library-only; no map/GPS anywhere.
    pub(crate) fn draw_library_people(
        &mut self,
        ctx: &crate::egui::Context,
        ui: &mut crate::egui::Ui,
    ) {
        ui.heading(Str::FacePeople.t());
        let status = self.face_view_status();
        ui.label(Str::FaceStatusPattern.format_arg(status.text()));
        if status == FaceViewStatus::NoAnalysis {
            ui.label(Str::FaceNoAnalysisHint.t());
            return;
        }
        ui.horizontal(|ui| {
            ui.label(Str::FaceFilterLabel.t());
            let mut filter = self.people_filter.clone();
            if ui
                .add(
                    crate::egui::TextEdit::singleline(&mut filter)
                        .hint_text(Str::FaceFilterHint.t()),
                )
                .changed()
            {
                self.people_filter = filter.clone();
            }
        });
        if status == FaceViewStatus::Stale {
            ui.colored_label(crate::theme::ACCENT, Str::FaceStaleWarning.t());
        }
        if status == FaceViewStatus::Missing {
            ui.colored_label(crate::theme::ACCENT, Str::FaceMissingWarning.t());
        }
        let Ok(analysis) = self.current_face_analysis() else {
            return;
        };
        let filter = self.people_filter.trim().to_lowercase();
        let person_for_cluster = |cluster: &FaceCluster| -> Option<String> {
            analysis
                .persons
                .iter()
                .find(|person| person.cluster_ids.iter().any(|id| id == &cluster.id))
                .map(|person| person.name.clone())
        };
        let mut selected = self.people_selected_cluster.clone();
        for cluster in &analysis.clusters {
            let name = person_for_cluster(cluster);
            if !filter.is_empty()
                && !name
                    .as_deref()
                    .is_some_and(|name| name.to_lowercase().contains(&filter))
            {
                continue;
            }
            let label = Str::FaceClusterPattern.format_arg(&format!(
                "{}  ({} face(s))",
                name.unwrap_or_else(|| "-".to_string()),
                cluster.detection_ids.len()
            ));
            if ui.selectable_label(selected == cluster.id, label).clicked() {
                selected = cluster.id.clone();
            }
        }
        self.people_selected_cluster = selected.clone();
        ui.separator();
        if selected.is_empty() {
            ui.label(Str::FaceSelectCluster.t());
            return;
        }
        // Explicit name assignment.
        ui.horizontal(|ui| {
            let mut name = self.people_name_input.clone();
            if ui
                .add(crate::egui::TextEdit::singleline(&mut name).hint_text(Str::FaceNameHint.t()))
                .changed()
            {
                self.people_name_input = name.clone();
            }
            if ui.button(Str::FaceConfirm.t()).clicked() {
                let name = self.people_name_input.trim().to_string();
                if name.is_empty() {
                    self.show_error(Str::FaceNameRequired.t());
                } else if let Err(error) = self.face_confirm_person(&selected, &name) {
                    self.show_error(error);
                } else {
                    self.status = Str::FaceConfirmDonePattern.format_arg(&name);
                }
            }
        });
        // Split: comma-separated detection ids of the selected cluster.
        ui.horizontal(|ui| {
            let mut subset = self.people_split_subset.clone();
            if ui
                .add(
                    crate::egui::TextEdit::singleline(&mut subset)
                        .hint_text(Str::FaceSplitHint.t()),
                )
                .changed()
            {
                self.people_split_subset = subset.clone();
            }
            if ui.button(Str::FaceSplit.t()).clicked() {
                let ids: Vec<String> = self
                    .people_split_subset
                    .split(',')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .collect();
                match self.face_split_cluster(&selected, &ids) {
                    Ok(()) => self.status = Str::FaceSplitDone.t().into(),
                    Err(error) => self.show_error(error),
                }
            }
        });
        // Merge: second cluster id (the selected cluster keeps its id).
        ui.horizontal(|ui| {
            let mut target = self.people_merge_target.clone();
            if ui
                .add(
                    crate::egui::TextEdit::singleline(&mut target)
                        .hint_text(Str::FaceMergeHint.t()),
                )
                .changed()
            {
                self.people_merge_target = target.clone();
            }
            if ui.button(Str::FaceMerge.t()).clicked() {
                let second = self.people_merge_target.trim().to_string();
                match self.face_merge_clusters(&selected, &second) {
                    Ok(()) => self.status = Str::FaceMergeDone.t().into(),
                    Err(error) => self.show_error(error),
                }
            }
        });
        self.draw_face_crop_strip(ctx, ui, &analysis, &selected);
    }

    fn draw_face_crop_strip(
        &mut self,
        ctx: &crate::egui::Context,
        ui: &mut crate::egui::Ui,
        analysis: &FaceAnalysis,
        cluster_id: &str,
    ) {
        let Some(cluster) = analysis.clusters.iter().find(|c| c.id == cluster_id) else {
            return;
        };
        let Some(frame) = self.original.clone() else {
            return;
        };
        let person_name = analysis
            .persons
            .iter()
            .find(|person| person.cluster_ids.iter().any(|id| id == cluster_id))
            .map(|person| person.name.clone());
        ui.separator();
        ui.label(Str::FaceCrops.t());
        // FACE-20 §3 Develop bridge: one action per detected face turns the
        // persisted box into a mask source on the active copy. Deferred so the
        // creation (a sidecar write) runs after the crop strip's borrows end.
        let mut mask_requests: Vec<(String, String)> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            for (index, detection_id) in cluster.detection_ids.iter().enumerate() {
                let Some(detection) = analysis.detections.iter().find(|d| &d.id == detection_id)
                else {
                    continue;
                };
                let key = format!("{}|{}", self.path, detection.id);
                let texture = self.face_crop_textures.get(&key).cloned().or_else(|| {
                    let crop = face_crop_frame(&frame, &detection.bbox, 96)?;
                    Some(ctx.load_texture(
                        key.clone(),
                        crate::egui::ColorImage::from_rgba_unmultiplied(
                            [crop.width as usize, crop.height as usize],
                            &crop.pixels,
                        ),
                        crate::egui::TextureOptions::LINEAR,
                    ))
                });
                if let Some(texture) = texture {
                    self.face_crop_textures.insert(key, texture.clone());
                    let name = match &person_name {
                        Some(person) => format!("{person} face {}", index + 1),
                        None => format!("Face {}", index + 1),
                    };
                    ui.vertical(|ui| {
                        ui.image((texture.id(), crate::egui::vec2(96.0, 96.0)));
                        if ui.button(Str::FaceUseAsMask.t()).clicked() {
                            mask_requests.push((detection.id.clone(), name));
                        }
                    });
                }
            }
        });
        for (detection_id, name) in mask_requests {
            match self.create_face_mask(&detection_id, name) {
                Ok(_) => self.status = Str::FaceMaskCreated.t().into(),
                Err(error) => self.show_error(error),
            }
        }
    }
}

/// Real evidence about the binary face artifacts a persisted analysis
/// references, derived from the `.lumina.zdata` records themselves — never
/// from mere existence.
///
/// This delegates to the shared `lumina_sidecar::face_artifact_evidence`
/// contract (FACE-20 §3.2), used verbatim by the CLI too, so both classify the
/// same sidecar identically:
///
/// - an analysis with no persisted embedding references, a missing bundle, a
///   missing `face_embedding` record or an unreadable file →
///   [`FaceArtifactEvidence::Missing`]
/// - a present record whose record checksum differs from the persisted
///   `FaceVectorRef.checksum`, or whose dimension differs, or a bundle that is
///   not loadable → `Present { checksum_matches: false }` (classified
///   `corrupt`)
/// - every referenced record verified → `Present { checksum_matches: true }`
fn face_artifact_evidence(bundle_root: &Path, analysis: &FaceAnalysis) -> FaceArtifactEvidence {
    lumina_sidecar::face_artifact_evidence(bundle_root, analysis)
}

/// Crops the normalized face box out of `frame` and downscales it to
/// `max_dim` px. Pure geometry/IO-free helper; `None` for degenerate boxes.
pub fn face_crop_frame(
    frame: &lumina_core::ImageFrame,
    bbox: &FaceBoundingBox,
    max_dim: u32,
) -> Option<lumina_core::ImageFrame> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let clamp01 = |value: f32| value.clamp(0.0, 1.0);
    let x = (clamp01(bbox.x) * frame.width as f32).floor() as u32;
    let y = (clamp01(bbox.y) * frame.height as f32).floor() as u32;
    let width = (clamp01(bbox.width) * frame.width as f32).ceil() as u32;
    let height = (clamp01(bbox.height) * frame.height as f32).ceil() as u32;
    if width == 0 || height == 0 || x >= frame.width || y >= frame.height {
        return None;
    }
    let width = width.min(frame.width - x);
    let height = height.min(frame.height - y);
    let cropped = frame.crop_region(x, y, width, height).ok()?;
    lumina_core::downscale_bilinear(&cropped, max_dim).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::{ImageFileFormat, ImageFrame};
    use lumina_onnx::{
        face_identity, DetectedFace, FaceAnalysisOutput, FaceClusteringParams, FaceEmbeddingRecord,
        FaceEmbeddingVector, FaceModelSuite,
    };
    use lumina_sidecar::{
        validate_face_analysis, DecodeFingerprint, Extras, FaceLandmark, FaceVectorRef,
        GeometryFingerprint, SourceFingerprint,
    };
    use std::path::Path;

    fn new_app() -> LuminaApp {
        LuminaApp::new(crate::egui::Context::default())
    }

    fn save_png(path: &Path) {
        let pixels: Vec<u8> = (0..64 * 48)
            .flat_map(|i| {
                [
                    (i % 200) as u8,
                    ((i * 3) % 200) as u8,
                    ((i * 7) % 200) as u8,
                    255,
                ]
            })
            .collect();
        let png = ImageFrame::new(64, 48, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    fn open_and_decode(app: &mut LuminaApp, path: &Path) {
        app.open_file(path.display().to_string());
        for _ in 0..2000 {
            app.poll_decode();
            if app.original.is_some() || app.error().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(app.error().is_none(), "decode failed: {:?}", app.error());
    }

    /// Minimal valid face analysis: two detections, two clusters. The identity
    /// is built from the *live* loaded source context so the status starts
    /// `valid` (a hand-written hash would be a false `stale`).
    fn face_analysis(
        source: SourceFingerprint,
        decode: DecodeFingerprint,
        geometry: GeometryFingerprint,
    ) -> FaceAnalysis {
        let identity = face_identity(
            &FaceModelSuite::candidate(),
            source,
            decode,
            geometry,
            FaceClusteringParams::default().to_identity(),
            &lumina_onnx::FaceInferenceOptions::default(),
        )
        .unwrap();
        let detection = |name: &str, x: f32| DetectedFace {
            bbox: FaceBoundingBox {
                x,
                y: 0.2,
                width: 0.2,
                height: 0.2,
            },
            score: 0.9,
            landmarks: vec![FaceLandmark {
                name: name.into(),
                x,
                y: 0.25,
            }],
        };
        let detections = vec![detection("left_eye", 0.1), detection("left_eye", 0.5)];
        let records = face_vector_records();
        let embeddings = detections
            .iter()
            .enumerate()
            .map(|(index, _)| FaceEmbeddingRecord {
                detection_index: index,
                vector: FaceEmbeddingVector::new(if index == 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                })
                .unwrap(),
                reference: FaceVectorRef {
                    relative_path: "photo.png.lumina.zdata".into(),
                    format: "lumina-zdata".into(),
                    // Exactly the digest of the record written by `seed_face`:
                    // a fabricated checksum would (correctly) read `corrupt`.
                    checksum: records[index].checksum(),
                    dimension: 2,
                    channels: "f32".into(),
                    data_version: "1".into(),
                    extras: Extras::new(),
                },
            })
            .collect();
        let ids: Vec<String> = detections
            .iter()
            .map(lumina_onnx::detected_face_id)
            .collect();
        FaceAnalysisOutput {
            identity,
            created_at: lumina_sidecar::now_rfc3339_utc(),
            detections,
            embeddings,
            clusters: vec![
                FaceCluster {
                    id: "cluster-a".into(),
                    detection_ids: vec![ids[0].clone()],
                    extras: Extras::new(),
                },
                FaceCluster {
                    id: "cluster-b".into(),
                    detection_ids: vec![ids[1].clone()],
                    extras: Extras::new(),
                },
            ],
            persons: vec![],
        }
        .into_sidecar()
        .unwrap()
    }

    /// Builds the exact `face_embedding` records `face_analysis` references.
    fn face_vector_records() -> Vec<lumina_sidecar::FaceEmbeddingArtifact> {
        [(0.1f32, vec![1.0f32, 0.0]), (0.5, vec![0.0, 1.0])]
            .into_iter()
            .map(|(x, values)| lumina_sidecar::FaceEmbeddingArtifact {
                id: lumina_onnx::embedding_id_for(&lumina_onnx::detected_face_id(&DetectedFace {
                    bbox: FaceBoundingBox {
                        x,
                        y: 0.2,
                        width: 0.2,
                        height: 0.2,
                    },
                    score: 0.9,
                    landmarks: vec![FaceLandmark {
                        name: "left_eye".into(),
                        x,
                        y: 0.25,
                    }],
                })),
                dimension: 2,
                values,
            })
            .collect()
    }

    fn seed_face(app: &mut LuminaApp) {
        app.ensure_document_loaded().unwrap();
        // The fixture's vector references point into the source bundle; write
        // the exact `face_embedding` records whose checksums the references
        // store, so the status is `valid` because the record checksum matches
        // (not by existence).
        lumina_sidecar::save_face_embeddings(
            &lumina_sidecar::zdata_path_for(Path::new(&app.path)),
            face_vector_records(),
        )
        .unwrap();
        let (source, decode, geometry) = {
            let source = &app.document.as_ref().unwrap().source;
            (
                SourceFingerprint {
                    content_hash: source.content_hash.clone(),
                    byte_length: source.byte_length,
                    extras: Extras::new(),
                },
                source.decode_fingerprint.clone(),
                source.geometry_fingerprint.clone(),
            )
        };
        let analysis = face_analysis(source, decode, geometry);
        validate_face_analysis(&analysis).unwrap();
        let document = app.document.as_mut().unwrap();
        document.face = Some(analysis);
        app.save_sidecar();
        assert!(app.error().is_none(), "{:?}", app.error());
    }

    #[test]
    fn person_ids_are_stable_and_name_derived() {
        assert_eq!(face_person_id_for("Alex"), face_person_id_for("Alex"));
        assert_ne!(face_person_id_for("Alex"), face_person_id_for("Sam"));
        assert!(face_person_id_for("Alex").starts_with("person-"));
        // Surrounding whitespace is not part of the identity.
        assert_eq!(face_person_id_for(" Alex "), face_person_id_for("Alex"));
        // Split ids are content-derived and order-independent.
        let a = vec!["d1".to_string(), "d2".to_string()];
        let b = vec!["d2".to_string(), "d1".to_string()];
        assert_eq!(face_split_cluster_id(&a), face_split_cluster_id(&b));
    }

    /// E2E (DoD §1): explicit confirm writes the name to the sidecar and a
    /// reopen shows it; the recipe/rating/flag/label are untouched.
    #[test]
    fn confirm_names_a_cluster_and_persists() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        let recipe = app.recipe().clone();
        app.face_confirm_person("cluster-a", "Alex").unwrap();
        let document = lumina_sidecar::load_sidecar(&sidecar_path_for(&source)).unwrap();
        let analysis = document.face.expect("face persisted");
        assert_eq!(analysis.persons.len(), 1);
        assert_eq!(analysis.persons[0].name, "Alex");
        assert!(analysis.persons[0].confirmed);
        assert_eq!(analysis.persons[0].id, face_person_id_for("Alex"));
        assert_eq!(document.virtual_copies[0].recipe, recipe);
        assert_eq!(document.virtual_copies[0].rating, 0);

        let mut reopened = new_app();
        open_and_decode(&mut reopened, &source);
        assert_eq!(reopened.face_person_names(), vec!["Alex".to_string()]);
        assert_eq!(reopened.face_view_status(), FaceViewStatus::Valid);
    }

    /// Split/merge are the pure onnx data operations and roundtrip.
    #[test]
    fn split_and_merge_clusters_persist() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        app.face_merge_clusters("cluster-a", "cluster-b").unwrap();
        let merged = app.face_analysis().unwrap();
        assert_eq!(merged.clusters.len(), 1);
        assert_eq!(merged.clusters[0].id, "cluster-a");
        assert_eq!(merged.clusters[0].detection_ids.len(), 2);

        let subset = vec![merged.clusters[0].detection_ids[1].clone()];
        app.face_split_cluster("cluster-a", &subset).unwrap();
        let split = app.face_analysis().unwrap();
        assert_eq!(split.clusters.len(), 2);
        let new_id = face_split_cluster_id(&subset);
        assert!(split.clusters.iter().any(|c| c.id == new_id));
        let persisted = lumina_sidecar::load_sidecar(&sidecar_path_for(&source)).unwrap();
        assert_eq!(persisted.face.unwrap().clusters.len(), 2);

        // Loud failures (empty subset / unknown cluster) never write.
        assert!(app.face_split_cluster("cluster-a", &[]).is_err());
        assert!(app.face_merge_clusters("cluster-a", "nope").is_err());
    }

    /// FACE-20 §4: a changed source is `stale`; a missing vector bundle is
    /// `missing`; both are surfaced, never silently re-run.
    #[test]
    fn status_warnings_cover_stale_and_missing() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        // A matching identity with present references is `valid`.
        assert_eq!(app.face_view_status(), FaceViewStatus::Valid);
        // A stored identity that does not match the live source → stale.
        {
            let document = app.document.as_mut().unwrap();
            let analysis = document.face.as_mut().unwrap();
            analysis.identity.source.content_hash = "blake3:other".into();
        }
        app.save_sidecar();
        assert!(app.error().is_none(), "{:?}", app.error());
        let mut reopened = new_app();
        open_and_decode(&mut reopened, &source);
        assert_eq!(reopened.face_view_status(), FaceViewStatus::Stale);
        // A missing referenced bundle → missing (never silently re-run).
        std::fs::remove_file(lumina_sidecar::zdata_path_for(&source)).unwrap();
        assert_eq!(reopened.face_view_status(), FaceViewStatus::Missing);
        assert_eq!(FaceViewStatus::Missing.text(), Str::FaceStatusMissing.t());
    }

    /// No analysis is a valid state, not an error.
    #[test]
    fn no_analysis_is_valid_state() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        assert_eq!(app.face_view_status(), FaceViewStatus::NoAnalysis);
        assert!(app.face_person_names().is_empty());
        assert!(app.face_confirm_person("cluster-a", "Alex").is_err());
        // The Develop bridge is loud without an analysis.
        assert!(app.create_face_mask("any", "Face 1").is_err());
    }

    /// The `person:` Library filter uses the scanned names.
    #[test]
    fn library_filter_supports_person_tokens() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        app.face_confirm_person("cluster-a", "Alex").unwrap();
        let entry = app
            .entries()
            .iter()
            .find(|entry| entry.path.display().to_string() == app.path)
            .cloned()
            .expect("scan entry");
        assert!(crate::library_entry_matches(&entry, "person:Alex"));
        assert!(!crate::library_entry_matches(&entry, "person:Sam"));
        assert!(!crate::library_entry_matches(&entry, "person:"));
    }

    /// The People view paints the cluster list, the filter row and the status
    /// once an analysis exists (headless, no GPU).
    #[test]
    fn people_view_paints_clusters_and_filter() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        app.set_library_view(crate::LibraryView::People);
        app.people_selected_cluster = "cluster-a".into();
        let ctx = crate::egui::Context::default();
        let raw = crate::egui::RawInput {
            screen_rect: Some(crate::egui::Rect::from_min_size(
                crate::egui::pos2(0.0, 0.0),
                crate::egui::vec2(1200.0, 4096.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            app.draw_library_grid(&ctx, ui)
        });
        output.textures_delta.clear();
        let painted = |needle: &str| {
            output.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, crate::egui::Shape::Text(text) if text.galley.text() == needle)
            })
        };
        assert!(painted(Str::FacePeople.t()));
        assert!(painted(
            &Str::FaceStatusPattern.format_arg(Str::FaceStatusValid.t())
        ));
        assert!(painted(Str::FaceFilterLabel.t()));
        assert!(painted(Str::FaceCrops.t()));
        assert!(painted(Str::FaceConfirm.t()));
        // FACE-20 §3 Develop bridge: the per-face mask-source action paints.
        assert!(painted(Str::FaceUseAsMask.t()));
    }

    /// F2/F4/B2: a tampered vector record payload is classified `corrupt`
    /// from the real BLAKE3 record checksum (not presence-only), and the
    /// Develop bridge refuses a `corrupt` analysis loudly.
    #[test]
    fn tampered_vector_bundle_is_corrupt() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        let detection = app.face_analysis().unwrap().detections[0].id.clone();
        assert_eq!(app.face_view_status(), FaceViewStatus::Valid);
        // The file still exists, but its digest no longer matches the persisted
        // checksum → corrupt, never a false `valid`.
        std::fs::write(lumina_sidecar::zdata_path_for(&source), b"tampered").unwrap();
        assert_eq!(app.face_view_status(), FaceViewStatus::Corrupt);
        assert_eq!(FaceViewStatus::Corrupt.text(), Str::FaceStatusCorrupt.t());
        // B2: the bridge refuses the corrupt analysis loudly (status gate).
        let error = app.create_face_mask(&detection, "Corrupt").unwrap_err();
        assert!(
            error.to_string().contains(Str::FaceStatusCorrupt.t()),
            "corrupt analysis must be refused loudly, got: {error}"
        );
    }

    /// F2: an analysis whose vectors were never persisted has no verifiable
    /// payload and reports `missing` — never a false `valid` from an empty
    /// reference set.
    #[test]
    fn vectors_never_persisted_report_missing_not_valid() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        // Detections remain, but no binary vector payload is referenced.
        {
            let document = app.document.as_mut().unwrap();
            document.face.as_mut().unwrap().embeddings.clear();
        }
        assert_eq!(app.face_view_status(), FaceViewStatus::Missing);
    }

    /// F1 (FACE-20 §3 Develop bridge): a persisted, valid face box becomes a
    /// deterministic mask source on the active copy; stale/missing/unknown
    /// inputs are loud refusals.
    #[test]
    fn face_mask_bridge_is_deterministic_and_loud() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, &source);
        seed_face(&mut app);
        let detection = app.face_analysis().unwrap().detections[0].clone();
        let mask_id = app.create_face_mask(&detection.id, "Face 1").unwrap();
        {
            let document = app.document.as_ref().unwrap();
            let definition = document.virtual_copies[0]
                .mask_library
                .iter()
                .find(|mask| mask.id == mask_id)
                .expect("bridge created the mask");
            assert_eq!(definition.status, MaskStatus::Valid);
            assert!(definition.ai_select.is_none(), "not the generic model path");
            assert!(definition.references.is_empty());
            match definition.prompt.as_ref().expect("box prompt") {
                MaskPrompt::Box {
                    rect,
                    transformation,
                } => {
                    assert!((rect.x - detection.bbox.x).abs() < 1e-6);
                    assert!((rect.y - detection.bbox.y).abs() < 1e-6);
                    assert!((rect.width - detection.bbox.width).abs() < 1e-6);
                    assert!((rect.height - detection.bbox.height).abs() < 1e-6);
                    assert_eq!(transformation.method, "face-detection");
                    assert_eq!(
                        transformation
                            .parameters
                            .get("detection_id")
                            .map(String::as_str),
                        Some(detection.id.as_str())
                    );
                    assert!(transformation
                        .parameters
                        .contains_key("face_identity_digest"));
                }
                other => panic!("expected a face box prompt, got {other:?}"),
            }
        }
        // Determinism: the same detection derives the same id, so a second
        // bridge run is refused instead of duplicating or overwriting.
        assert!(app.create_face_mask(&detection.id, "Face 1 copy").is_err());
        // Persisted: a reopen restores the mask source.
        let mut reopened = new_app();
        open_and_decode(&mut reopened, &source);
        assert!(reopened.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .iter()
            .any(|mask| mask.id == mask_id));

        // Unknown detection id is loud. This must run on a `valid` analysis so
        // the status gate passes and the error really comes from the detection
        // lookup (`FaceDetectionNotFound`), not from the gate above.
        let error = app
            .create_face_mask("does-not-exist", "Unknown")
            .unwrap_err();
        assert!(
            error.to_string().contains(Str::FaceDetectionNotFound.t()),
            "unknown detection must be named loudly, got: {error}"
        );

        // Loud on a stale analysis (the status gate refuses before any lookup).
        {
            let document = app.document.as_mut().unwrap();
            document.face.as_mut().unwrap().identity.source.content_hash = "blake3:other".into();
        }
        app.save_sidecar();
        assert_eq!(app.face_view_status(), FaceViewStatus::Stale);
        let error = app.create_face_mask(&detection.id, "Stale").unwrap_err();
        assert!(
            error.to_string().contains(Str::FaceStatusStale.t()),
            "stale analysis must be refused loudly, got: {error}"
        );
        // Loud on a missing referenced artifact.
        std::fs::remove_file(lumina_sidecar::zdata_path_for(&source)).unwrap();
        assert_eq!(app.face_view_status(), FaceViewStatus::Missing);
        let error = app.create_face_mask(&detection.id, "Missing").unwrap_err();
        assert!(
            error.to_string().contains(Str::FaceStatusMissing.t()),
            "missing artifact must be refused loudly, got: {error}"
        );
    }

    /// The crop helper maps the normalized box into frame pixels.
    #[test]
    fn face_crop_frame_maps_normalized_box() {
        let pixels: Vec<u8> = (0..64 * 48).flat_map(|_| [10u8, 20, 30, 255]).collect();
        let frame = ImageFrame::new(64, 48, pixels).unwrap();
        let bbox = FaceBoundingBox {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        };
        let crop = face_crop_frame(&frame, &bbox, 96).unwrap();
        assert_eq!((crop.width, crop.height), (32, 24));
        let degenerate = FaceBoundingBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        assert!(face_crop_frame(&frame, &degenerate, 96).is_none());
    }
}
