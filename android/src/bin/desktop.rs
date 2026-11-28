//! The Android app's UI and store in a desktop window, for development:
//! `cargo run`, with the data in `./beep-data` (or `$BEEP_DATA`).

fn main() -> eframe::Result {
    env_logger::init();
    let dir = std::env::var_os("BEEP_DATA").unwrap_or_else(|| "beep-data".into());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Beep Test",
        options,
        Box::new(move |_cc| Ok(Box::new(beep_android::app(std::path::Path::new(&dir))?))),
    )
}
