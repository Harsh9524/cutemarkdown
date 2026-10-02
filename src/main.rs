// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use engine::{DocView, Document, Style};

struct App {
    view: Option<DocView>,
    style: Style,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.style.palette.bg))
            .show(ui, |ui| match &mut self.view {
                Some(view) => {
                    view.show(ui, &self.style);
                }
                None => {
                    ui.centered_and_justified(|ui| ui.label("Drop a Markdown file here"));
                }
            });
    }
}

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let view = path.as_ref().and_then(|p| {
        let src = std::fs::read_to_string(p).ok()?;
        Some(DocView::new(Document::parse(&src, p.parent())))
    });
    eframe::run_native(
        "cutemarkdown",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            engine::fonts::install(&cc.egui_ctx);
            Ok(Box::new(App { view, style: Style::default() }))
        }),
    )
}
