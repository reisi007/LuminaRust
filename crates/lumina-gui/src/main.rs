#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {}

#[cfg(not(target_arch = "wasm32"))]
mod logger;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    logger::install_panic_hook();
    // R2-GUIMOD-07: `Info` is the default so the per-line format!/Mutex/
    // eprintln cost and the panic ring stay free of wgpu/naga/egui trace
    // chatter. Verbose diagnosis is one env var away:
    //   RUST_LOG=trace lumina-gui <dir> 2> gui.log
    let _level = logger::init_logging(log::LevelFilter::Info);
    let workdir = parse_workdir();
    // GUI-WGPU-PRESENT-1: the wgpu renderer shares its Device/Queue with
    // `lumina-gpu` (via `CreationContext::wgpu_render_state`), so the preview
    // presents straight from VRAM without any CPU readback. The renderer is
    // set explicitly to document the dependency of that path.
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Lumina",
        options,
        Box::new(move |creation_context| {
            let mut app = lumina_gui::LuminaApp::new(creation_context.egui_ctx.clone());
            lumina_gui::attach_wgpu_render_state(
                &mut app,
                creation_context.wgpu_render_state.clone(),
            );
            if let Some(dir) = workdir {
                app.set_directory(dir);
            }
            Ok(Box::new(app))
        }),
    )
}

/// Parse an optional working-directory argument. The first positional,
/// non-flag argument that resolves to an existing directory is treated as
/// the initial workdir for the GUI (F-???-GUI-CLI: command-line workdir
/// override). Flags (`-h`/`--help`, `-v`/`--version`, and any recognized
/// long option) are ignored here so they don't accidentally bind.
#[cfg(not(target_arch = "wasm32"))]
fn parse_workdir() -> Option<String> {
    let args = std::env::args().skip(1);
    for arg in args {
        if arg.starts_with('-') {
            // Skip the value following a `--flag=value`-less long option's
            // potential companion, but for simplicity only consume the next
            // non-flag when this flag itself might take one. We don't define
            // any value-taking flags today, so just continue.
            continue;
        }
        let path = std::path::Path::new(&arg);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(path)
        };
        if candidate.is_dir() {
            return Some(candidate.display().to_string());
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn main() {}
