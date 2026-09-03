mod logger;

use lumina_gui::Module;

fn main() -> eframe::Result {
    logger::install_panic_hook();
    // R2-GUIMOD-07: `Info` is the default so the per-line format!/Mutex/
    // eprintln cost and the panic ring stay free of wgpu/naga/egui trace
    // chatter. Verbose diagnosis is one env var away:
    //   RUST_LOG=trace lumina-gui <dir> 2> gui.log
    let _level = logger::init_logging(log::LevelFilter::Info);
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_startup_args(&raw_args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("{STARTUP_USAGE}");
            std::process::exit(2);
        }
    };
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
            #[cfg(feature = "gpu")]
            lumina_gui::attach_wgpu_render_state(
                &mut app,
                creation_context.wgpu_render_state.clone(),
            );
            // GUI-STARTUP-MODULEFLAGS-1 (F-100 Startverhalten): deterministic
            // start state from CLI flags — no session persistence in v1.
            // Neither setter touches recipe or sidecar (display-only state).
            app.set_module(config.module);
            app.set_fullscreen(config.fullscreen);
            if let Some(dir) = config.workdir {
                app.set_directory(dir);
            }
            Ok(Box::new(app))
        }),
    )
}

/// Usage line printed to stderr when `--module` is unusable.
const STARTUP_USAGE: &str =
    "usage: lumina-gui [directory] [--module library|develop|export] [--fullscreen]";

/// Deterministic GUI start state (F-100 Startverhalten, v1 — no session
/// persistence, sidecar-first): an optional workdir override plus the start
/// module and the fullscreen working view. Defaults: no workdir override,
/// Develop, no fullscreen.
#[derive(Debug)]
struct StartupConfig {
    workdir: Option<String>,
    module: Module,
    fullscreen: bool,
}

/// Parse the GUI startup arguments (pure function over the post-`argv[0]`
/// args, so headless tests can drive it without touching the process
/// environment).
///
/// * First positional, non-flag argument that resolves to an existing
///   directory is the initial workdir (legacy `parse_workdir` behaviour).
/// * `--module library|develop|export` (also `--module=value`) selects the
///   start module; anything else is a loud error (exit 2 with usage).
/// * `--fullscreen` starts in the Lights-Out working view (same hidden chrome
///   as `L`, settled on Fit) — never OS-level fullscreen.
/// * Other flags (`-h`/`--help`, `-v`/`--version`, unrecognized long options)
///   are ignored here so they don't accidentally bind.
fn parse_startup_args(args: &[String]) -> Result<StartupConfig, String> {
    let mut config = StartupConfig {
        workdir: None,
        module: Module::Develop,
        fullscreen: false,
    };
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--fullscreen" {
            config.fullscreen = true;
        } else if arg == "--module" {
            index += 1;
            let value = args.get(index).ok_or_else(|| {
                "missing value for --module (expected library|develop|export)".to_string()
            })?;
            config.module = parse_module_value(value)?;
        } else if let Some(value) = arg.strip_prefix("--module=") {
            config.module = parse_module_value(value)?;
        } else if arg.starts_with('-') {
            // Ignored (see doc comment above).
        } else if config.workdir.is_none() {
            let path = std::path::Path::new(arg);
            let candidate = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path))
                    .unwrap_or_else(|_| path.to_path_buf())
            };
            if candidate.is_dir() {
                config.workdir = Some(candidate.display().to_string());
            }
        }
        index += 1;
    }
    Ok(config)
}

/// Map one `--module` value to its start module. Anything outside
/// `library|develop|export` is rejected loudly — no silent default.
fn parse_module_value(value: &str) -> Result<Module, String> {
    match value {
        "library" => Ok(Module::Library),
        "develop" => Ok(Module::Develop),
        "export" => Ok(Module::Export),
        _ => Err(format!(
            "unknown --module '{value}' (expected library|develop|export)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// GUI-STARTUP-MODULEFLAGS-1: no flags means Develop without fullscreen.
    #[test]
    fn default_start_is_develop_without_fullscreen() {
        let config = parse_startup_args(&args(&[])).unwrap();
        assert_eq!(config.module, Module::Develop);
        assert!(!config.fullscreen);
        assert!(config.workdir.is_none());
    }

    /// GUI-STARTUP-MODULEFLAGS-1: every `--module` value selects its module.
    #[test]
    fn module_flag_selects_each_module() {
        for (value, expected) in [
            ("library", Module::Library),
            ("develop", Module::Develop),
            ("export", Module::Export),
        ] {
            let config = parse_startup_args(&args(&["--module", value])).unwrap();
            assert_eq!(config.module, expected, "--module {value}");
        }
    }

    /// GUI-STARTUP-MODULEFLAGS-1: the `--module=value` form works too.
    #[test]
    fn module_equals_form_selects_module() {
        let config = parse_startup_args(&args(&["--module=export"])).unwrap();
        assert_eq!(config.module, Module::Export);
    }

    /// GUI-STARTUP-MODULEFLAGS-1: `--fullscreen` arms the working view.
    #[test]
    fn fullscreen_flag_sets_working_view() {
        let config = parse_startup_args(&args(&["--fullscreen"])).unwrap();
        assert!(config.fullscreen);
        assert_eq!(config.module, Module::Develop);
    }

    /// GUI-STARTUP-MODULEFLAGS-1: flags combine with a positional workdir.
    #[test]
    fn flags_combine_with_positional_workdir() {
        let directory = tempfile::tempdir().unwrap();
        let dir = directory.path().display().to_string();
        let raw = vec![
            "--module".to_string(),
            "library".to_string(),
            "--fullscreen".to_string(),
            dir,
        ];
        let config = parse_startup_args(&raw).unwrap();
        assert_eq!(config.module, Module::Library);
        assert!(config.fullscreen);
        assert!(config.workdir.is_some());
    }

    /// GUI-STARTUP-MODULEFLAGS-1: an unknown module is a loud error (the
    /// caller prints it with usage on stderr and exits non-zero).
    #[test]
    fn unknown_module_is_an_error() {
        let error = parse_startup_args(&args(&["--module", "lightroom"])).unwrap_err();
        assert!(
            error.contains("lightroom") && error.contains("library|develop|export"),
            "error must name the value and the valid set, got: {error:?}"
        );
    }

    /// GUI-STARTUP-MODULEFLAGS-1: a missing `--module` value is a loud error.
    #[test]
    fn missing_module_value_is_an_error() {
        assert!(parse_startup_args(&args(&["--module"])).is_err());
        assert!(parse_startup_args(&args(&["--module="])).is_err());
    }
}
