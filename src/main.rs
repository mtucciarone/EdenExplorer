mod app;
mod state;
mod fs;
mod drives;

use app::ExplorerApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ExplorerEden",
        options,
        Box::new(|_cc| Ok(Box::new(ExplorerApp::default()))),
    )
}
