#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "Lumina",
        eframe::NativeOptions::default(),
        Box::new(|creation_context| {
            Ok(Box::new(lumina_gui::LuminaApp::new(
                creation_context.egui_ctx.clone(),
            )))
        }),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
