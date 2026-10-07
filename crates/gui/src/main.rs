#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 1024.0])
            .with_min_inner_size([400.0, 400.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(&include_bytes!("../../../assets/icon128.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "obamify",
        native_options,
        Box::new(|cc| Ok(Box::new(obamify_gui::ObamifyApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn start_app() {
    use eframe::wasm_bindgen::JsCast as _;
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    eframe::WebLogger::init(log::LevelFilter::Warn).ok();
    let web_options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().expect("No window").document().expect("No document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");
        let start_result = eframe::WebRunner::new()
            .start(canvas, web_options, Box::new(|cc| Ok(Box::new(obamify_gui::ObamifyApp::new(cc)))))
            .await;
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => { loading_text.remove(); }
                Err(e) => {
                    use web_sys::js_sys::JsString;
                    loading_text.set_inner_html(&format!(
                        "<div>Please enable hardware acceleration in your browser :)</div><div class=\"error\">Error: {}</div>",
                        std::convert::Into::<JsString>::into(e.clone())
                    ));
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
pub fn main() {
    use wasm_bindgen::JsCast as _;
    console_error_panic_hook::set_once();
    if web_sys::window().is_some() {
        start_app();
        return;
    }
    if web_sys::js_sys::global()
        .dyn_ref::<web_sys::DedicatedWorkerGlobalScope>()
        .is_some()
    {
        obamify_gui::worker_entry();
        return;
    }
    web_sys::console::warn_1(&"Unknown global".into());
}
