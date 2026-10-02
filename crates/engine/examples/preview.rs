//! Engine preview & screenshot harness (no app chrome).
//!
//! ```sh
//! cargo run -p cutemarkdown-engine --example preview -- samples/ai-report.md --theme dark
//! # headless screenshot (Linux): wraps in Xvfb
//! xvfb-run -a -s "-screen 0 1920x1200x24" cargo run -p cutemarkdown-engine --example preview -- \
//!     samples/ai-report.md --size 1100x900 --scroll 1200 --screenshot /tmp/shot.png
//! ```
//!
//! Flags: `--theme light|dark|sepia`, `--font sans|serif`, `--text-size 16`, `--measure 720`,
//! `--size WxH`, `--ppp 1.5` (pixels per point, e.g. Windows 150% scaling), `--scroll PX`,
//! `--anchor ID`, `--find QUERY`, `--screenshot OUT.png`, `--frames N` (frames before capture).

use eframe::egui;
use engine::{DocView, Document, FontChoice, Style, ThemeKind};

struct Args {
    file: std::path::PathBuf,
    theme: ThemeKind,
    font: FontChoice,
    text_size: f32,
    measure: f32,
    size: [f32; 2],
    ppp: Option<f32>,
    scroll: Option<f32>,
    anchor: Option<String>,
    find: Option<String>,
    screenshot: Option<String>,
    frames: u32,
}

fn parse_args() -> Args {
    let mut a = Args {
        file: "samples/ai-report.md".into(),
        theme: ThemeKind::Light,
        font: FontChoice::Sans,
        text_size: 16.0,
        measure: 720.0,
        size: [1100.0, 900.0],
        ppp: None,
        scroll: None,
        anchor: None,
        find: None,
        screenshot: None,
        frames: 12,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| panic!("missing value for {arg}"));
        match arg.as_str() {
            "--theme" => {
                a.theme = match val().as_str() {
                    "dark" => ThemeKind::Dark,
                    "sepia" => ThemeKind::Sepia,
                    _ => ThemeKind::Light,
                }
            }
            "--font" => a.font = if val() == "serif" { FontChoice::Serif } else { FontChoice::Sans },
            "--text-size" => a.text_size = val().parse().unwrap(),
            "--measure" => a.measure = val().parse().unwrap(),
            "--size" => {
                let v = val();
                let (w, h) = v.split_once('x').expect("--size WxH");
                a.size = [w.parse().unwrap(), h.parse().unwrap()];
            }
            "--ppp" => a.ppp = Some(val().parse().unwrap()),
            "--scroll" => a.scroll = Some(val().parse().unwrap()),
            "--anchor" => a.anchor = Some(val()),
            "--find" => a.find = Some(val()),
            "--screenshot" => a.screenshot = Some(val()),
            "--frames" => a.frames = val().parse().unwrap(),
            _ => a.file = arg.into(),
        }
    }
    a
}

struct Preview {
    view: DocView,
    style: Style,
    args: Args,
    frame: u32,
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.frame == 1 {
            if let Some(y) = self.args.scroll {
                self.view.set_scroll_offset(y);
            }
            if let Some(a) = &self.args.anchor {
                self.view.scroll_to_anchor(a);
            }
            if let Some(q) = &self.args.find {
                let st = self.view.set_find_query(q);
                eprintln!("find {q:?}: {st:?}");
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.style.palette.bg))
            .show(ui, |ui| {
                let out = self.view.show(ui, &self.style);
                if let Some(link) = out.clicked_link {
                    eprintln!("clicked: {link:?}");
                }
            });

        self.frame += 1;
        if let Some(path) = self.args.screenshot.clone() {
            if self.frame == self.args.frames {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = shot {
                let img = image::RgbaImage::from_raw(
                    image.width() as u32,
                    image.height() as u32,
                    image.as_raw().to_vec(),
                )
                .expect("screenshot buffer");
                img.save(&path).expect("save screenshot");
                eprintln!("saved {path} ({}x{})", image.width(), image.height());
                std::process::exit(0);
            }
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let args = parse_args();
    let source = std::fs::read_to_string(&args.file).expect("read markdown file");
    let t0 = std::time::Instant::now();
    let doc = Document::parse(&source, args.file.parent());
    eprintln!(
        "parsed {} bytes in {:?}: {} headings, {} words",
        source.len(),
        t0.elapsed(),
        doc.headings().len(),
        doc.word_count()
    );
    let style = Style::new(args.theme, args.font, args.text_size, args.measure);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(args.size).with_title("engine preview"),
        ..Default::default()
    };
    eframe::run_native(
        "engine-preview",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            engine::fonts::install(&cc.egui_ctx);
            if let Some(ppp) = args.ppp {
                cc.egui_ctx.set_pixels_per_point(ppp);
            }
            Ok(Box::new(Preview { view: DocView::new(doc), style, args, frame: 0 }))
        }),
    )
}
