//! GUI-REFACTOR-W2-20 S2.8: folder/catalog file operations, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::create_folder`], [`LuminaApp::rename_folder`],
//! [`LuminaApp::move_image_to_folder`], [`LuminaApp::delete_image_with_sidecars`]
//! and [`LuminaApp::delete_empty_folder`] implement the G-09 catalog management
//! ops. The non-destructive contract is unchanged: moves never overwrite
//! silently, sidecar companions travel with the image and empty-folder deletes
//! refuse non-empty directories (loud errors). All stay `pub`.

use super::*;
use log::info;

impl LuminaApp {
    /// Create a folder (G-09 catalog management). Loud when the target
    /// already exists as a non-directory or when creation fails; never
    /// touches recipes or sidecars (there is nothing to accompany yet).
    pub fn create_folder(&mut self, path: &Path) -> Result<(), GuiError> {
        if path.exists() {
            if !path.is_dir() {
                return Err(GuiError::Io(format!(
                    "cannot create folder `{}`: a file already exists",
                    path.display()
                )));
            }
            info!("folder already exists: {}", path.display());
            self.status = Str::FolderExistsPattern.format_arg(&path.display().to_string());
            return Ok(());
        }
        std::fs::create_dir_all(path).map_err(|error| {
            GuiError::Io(format!(
                "cannot create folder `{}`: {error}",
                path.display()
            ))
        })?;
        info!("folder created: {}", path.display());
        self.status = Str::FolderCreatedPattern.format_arg(&path.display().to_string());
        Ok(())
    }

    /// Rename (move) a folder (G-09 catalog management). Sidecars live next
    /// to their sources inside the folder, so they travel with the single
    /// directory `rename` — no per-file bookkeeping, no absolute paths.
    /// Loud when the source is missing or the target already exists.
    pub fn rename_folder(&mut self, from: &Path, to: &Path) -> Result<(), GuiError> {
        if !from.is_dir() {
            return Err(GuiError::Io(format!(
                "cannot rename folder `{}`: no such directory",
                from.display()
            )));
        }
        if to.exists() {
            return Err(GuiError::Io(format!(
                "cannot rename folder `{}` to `{}`: target already exists",
                from.display(),
                to.display()
            )));
        }
        std::fs::rename(from, to).map_err(|error| {
            GuiError::Io(format!(
                "cannot rename folder `{}`: {error}",
                from.display()
            ))
        })?;
        info!("folder renamed: {} -> {}", from.display(), to.display());
        self.status =
            Str::FolderRenamedPattern.format_arg(&format!("{} → {}", from.display(), to.display()));
        self.list_directory();
        Ok(())
    }

    /// Move one image plus its sidecar companions into `dest_dir` (G-09
    /// catalog management): `<name>.lumina.json` and `<name>.lumina.zdata`
    /// travel with the source when present; missing companions are no error.
    /// An existing target (image or companion) aborts loudly before anything
    /// is moved — never a silent overwrite. Each moved path logs `info!`.
    pub fn move_image_to_folder(
        &mut self,
        image: &Path,
        dest_dir: &Path,
    ) -> Result<PathBuf, GuiError> {
        if !image.is_file() {
            return Err(GuiError::Io(format!(
                "cannot move `{}`: no such file",
                image.display()
            )));
        }
        if !dest_dir.is_dir() {
            return Err(GuiError::Io(format!(
                "cannot move `{}`: destination `{}` is no directory",
                image.display(),
                dest_dir.display()
            )));
        }
        let file_name = image
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                GuiError::Io(format!("cannot move `{}`: no file name", image.display()))
            })?;
        let target = dest_dir.join(&file_name);
        if target.exists() {
            return Err(GuiError::Io(format!(
                "cannot move `{}` to `{}`: target already exists",
                image.display(),
                target.display()
            )));
        }
        for companion in [
            lumina_sidecar::sidecar_path_for(image),
            zdata_path_for(image),
        ] {
            if companion.is_file() {
                let companion_name = companion
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                if dest_dir.join(&companion_name).exists() {
                    return Err(GuiError::Io(format!(
                        "cannot move `{}` to `{}`: companion `{}` already exists",
                        image.display(),
                        target.display(),
                        companion_name
                    )));
                }
            }
        }
        let moved_loaded = Path::new(self.path.trim()) == image;
        // REVIEW-GUI-MOVE-1: flush an armed (uncommitted) edit to the sidecar
        // at the *current* path before the bundle moves, so the edit travels
        // with the image instead of being written to the old location later.
        // Only the loaded image has an armed edit to flush.
        if moved_loaded {
            self.flush_pending_edit();
        }
        move_file_cross_volume(image, &target)
            .map_err(|error| GuiError::Io(format!("cannot move `{}`: {error}", image.display())))?;
        info!("moved image: {} -> {}", image.display(), target.display());
        let mut companion_error: Option<String> = None;
        for companion in [
            lumina_sidecar::sidecar_path_for(image),
            zdata_path_for(image),
        ] {
            if companion.is_file() {
                let companion_name = companion
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                let companion_target = dest_dir.join(&companion_name);
                match move_file_cross_volume(&companion, &companion_target) {
                    Ok(()) => info!(
                        "moved sidecar companion: {} -> {}",
                        companion.display(),
                        companion_target.display()
                    ),
                    Err(error) => {
                        self.status = Str::CompanionMoveFailedPattern
                            .format_arg(&format!("{companion_name}: {error}"));
                        companion_error = Some(format!(
                            "moved `{}` to `{}` but companion `{}` failed: {error}",
                            image.display(),
                            target.display(),
                            companion.display()
                        ));
                        break;
                    }
                }
            }
        }
        // REVIEW-GUI-MOVE-1: when the loaded image itself moved, re-point the
        // session at the new path (and its sidecar revision) so the next
        // debounced save/refresh targets the moved bundle, never the old
        // location (which would orphan a sidecar / hit a CAS conflict). This
        // runs even when a companion move failed — the image is already at the
        // target, so the old path must not stay writable.
        if moved_loaded {
            self.path = target.display().to_string();
            self.sidecar_revision =
                lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&target))
                    .ok()
                    .and_then(|document| lumina_sidecar::document_revision(&document).ok());
            if let Some(parent) = target.parent() {
                self.directory = parent.display().to_string();
            }
        }
        if let Some(message) = companion_error {
            return Err(GuiError::Io(message));
        }
        self.status = Str::ImageMovedPattern.format_arg(&format!(
            "{} → {}",
            image.display(),
            target.display()
        ));
        self.list_directory();
        Ok(target)
    }

    /// Delete one image plus its sidecar companions (G-09 catalog
    /// management). Missing companions are no error; a missing image is a
    /// loud error. The selection stabilizes on the successor afterwards.
    pub fn delete_image_with_sidecars(&mut self, image: &Path) -> Result<(), GuiError> {
        if !image.is_file() {
            return Err(GuiError::Io(format!(
                "cannot delete `{}`: no such file",
                image.display()
            )));
        }
        let deleted_loaded = Path::new(self.path.trim()) == image;
        std::fs::remove_file(image).map_err(|error| {
            GuiError::Io(format!("cannot delete `{}`: {error}", image.display()))
        })?;
        info!("deleted image: {}", image.display());
        // REVIEW-GUI-MOVE-1: the loaded source is gone — drop any armed edit
        // and clear the path/revision so no later (debounced) save can
        // recreate an orphan sidecar at the deleted location. Done before the
        // companion loop: the image file is already removed, so a companion
        // failure must not leave the old path writable.
        if deleted_loaded {
            self.pending_slider_commit = None;
            self.pending_history_step = None;
            self.path.clear();
            self.sidecar_revision = None;
        }
        for companion in [
            lumina_sidecar::sidecar_path_for(image),
            zdata_path_for(image),
        ] {
            if companion.is_file() {
                std::fs::remove_file(&companion).map_err(|error| {
                    GuiError::Io(format!("cannot delete `{}`: {error}", companion.display()))
                })?;
                info!("deleted sidecar companion: {}", companion.display());
            }
        }
        self.status = Str::ImageDeletedPattern.format_arg(&image.display().to_string());
        self.list_directory();
        Ok(())
    }

    /// Delete an empty folder (G-09 catalog management). Non-empty folders
    /// are loudly refused (no recursive delete, no data loss); `.lumina`
    /// cache folders are never special-cased here — they delete like any
    /// other empty folder.
    pub fn delete_empty_folder(&mut self, path: &Path) -> Result<(), GuiError> {
        if !path.is_dir() {
            return Err(GuiError::Io(format!(
                "cannot delete folder `{}`: no such directory",
                path.display()
            )));
        }
        let is_empty = std::fs::read_dir(path)
            .map_err(|error| {
                GuiError::Io(format!("cannot read folder `{}`: {error}", path.display()))
            })?
            .next()
            .is_none();
        if !is_empty {
            return Err(GuiError::Io(format!(
                "cannot delete folder `{}`: directory is not empty",
                path.display()
            )));
        }
        std::fs::remove_dir(path).map_err(|error| {
            GuiError::Io(format!(
                "cannot delete folder `{}`: {error}",
                path.display()
            ))
        })?;
        info!("deleted empty folder: {}", path.display());
        self.status = Str::FolderDeletedPattern.format_arg(&path.display().to_string());
        self.list_directory();
        Ok(())
    }
}
