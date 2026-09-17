//! LRPAR-G12-FACE-20 (FACE-20-S5) — Library People view panel.
//!
//! Extracted verbatim from `face_gui.rs` (GUI-INSTRDBG-17c-Rest) so the
//! face/status module stays inside the file-size ratchet while the People-view
//! "Use as mask" bridge (`LuminaApp::create_face_mask`) gains its `GuiAction`
//! instrumentation. This module owns **no** face algorithm and no second
//! clustering path: it only paints the persisted `document.face` state (status,
//! cluster/person list, the explicit name/split/merge controls and the
//! face-crop strip) and defers the mask-source creation to the instrumented
//! `LuminaApp::create_face_mask`. Map/GPS is never read, persisted or displayed
//! here (FACE-20 §5).

use super::*;
use crate::face_gui::{face_crop_frame, FaceViewStatus};
use lumina_sidecar::{FaceAnalysis, FaceCluster};

impl LuminaApp {
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
