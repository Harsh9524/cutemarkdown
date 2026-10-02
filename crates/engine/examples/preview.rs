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
//! `--anchor ID`, `--find QUERY`, `--screenshot OUT.png`, `--frames N` (frames before capture),
//! `--wrap` (wrap long code lines), `--top-inset 44` (space for an app bar), `--select-all` (select everything, print the copy text),
//! `--reload FILE` (after the first frames, swap in FILE with `keep_position`, like live reload),
//! `--hover X,Y` (move the pointer there before capturing), `--click X,Y` / `--rclick X,Y`
//! (left/right click there once),
//! `--drag X,Y,X2,Y2` (drag-select), `--key NAME` (press a key once: PageDown, End, …),
//! `--bench` (scroll through the whole document and print layout and frame times; use a
//! release build: `cargo run --release -p cutemarkdown-engine --example preview -- FILE --bench`).

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
    wrap: bool,
    select_all: bool,
    reload: Option<std::path::PathBuf>,
    hover: Option<egui::Pos2>,
    click: Option<egui::Pos2>,
    rclick: Option<egui::Pos2>,
    drag: Option<(egui::Pos2, egui::Pos2)>,
    key: Option<egui::Key>,
    wheel: Option<(egui::Pos2, egui::Vec2)>,
    bench: bool,
    top_inset: f32,
}

fn pos(v: &str) -> egui::Pos2 {
    let (x, y) = v.split_once(',').expect("X,Y");
    egui::pos2(x.trim().parse().unwrap(), y.trim().parse().unwrap())
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
        wrap: false,
        select_all: false,
        reload: None,
        hover: None,
        click: None,
        rclick: None,
        drag: None,
        key: None,
        wheel: None,
        bench: false,
        top_inset: 0.0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || {
            it.next()
                .unwrap_or_else(|| panic!("missing value for {arg}"))
        };
        match arg.as_str() {
            "--theme" => {
                a.theme = match val().as_str() {
                    "dark" => ThemeKind::Dark,
                    "sepia" => ThemeKind::Sepia,
                    _ => ThemeKind::Light,
                }
            }
            "--font" => {
                a.font = if val() == "serif" {
                    FontChoice::Serif
                } else {
                    FontChoice::Sans
                }
            }
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
            "--wrap" => a.wrap = true,
            "--select-all" => a.select_all = true,
            "--reload" => a.reload = Some(val().into()),
            "--hover" => a.hover = Some(pos(&val())),
            "--click" => a.click = Some(pos(&val())),
            "--rclick" => a.rclick = Some(pos(&val())),
            "--drag" => {
                let v = val();
                let p: Vec<f32> = v.split(',').map(|s| s.trim().parse().unwrap()).collect();
                a.drag = Some((egui::pos2(p[0], p[1]), egui::pos2(p[2], p[3])));
            }
            "--key" => a.key = egui::Key::from_name(&val()),
            "--wheel" => {
                let v = val();
                let p: Vec<f32> = v.split(',').map(|s| s.trim().parse().unwrap()).collect();
                a.wheel = Some((egui::pos2(p[0], p[1]), egui::vec2(p[2], p[3])));
            }
            "--bench" => a.bench = true,
            "--top-inset" => a.top_inset = val().parse().unwrap(),
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        let f = self.frame;
        if let Some(p) = self.args.hover
            && f >= 2
        {
            raw.events.push(egui::Event::PointerMoved(p));
        }
        if let Some((p, button)) = self
            .args
            .click
            .map(|p| (p, egui::PointerButton::Primary))
            .or(self
                .args
                .rclick
                .map(|p| (p, egui::PointerButton::Secondary)))
        {
            let ev = |pressed| egui::Event::PointerButton {
                pos: p,
                button,
                pressed,
                modifiers: Default::default(),
            };
            match f {
                3 => raw.events.push(egui::Event::PointerMoved(p)),
                4 => raw.events.push(ev(true)),
                5 => raw.events.push(ev(false)),
                _ => {}
            }
        }
        if let Some((a, b)) = self.args.drag {
            let ev = |p, pressed| egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            match f {
                3 => raw.events.push(egui::Event::PointerMoved(a)),
                4 => raw.events.push(ev(a, true)),
                5 => raw
                    .events
                    .push(egui::Event::PointerMoved(a + (b - a) * 0.5)),
                6 => raw.events.push(egui::Event::PointerMoved(b)),
                7 => raw.events.push(ev(b, false)),
                _ => {}
            }
        }
        if let Some(key) = self.args.key
            && f == 4
        {
            for pressed in [true, false] {
                raw.events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
        }
        if let Some((p, delta)) = self.args.wheel {
            if f >= 2 {
                raw.events.push(egui::Event::PointerMoved(p));
            }
            if (4..8).contains(&f) {
                // Points, like a precision touchpad (no smoothing): DX scrolls sideways.
                raw.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: delta / 4.0,
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                });
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.frame == 1 {
            if let Some(y) = self.args.scroll {
                self.view.set_scroll_offset(y);
            }
            if let Some(a) = &self.args.anchor {
                let ok = self.view.scroll_to_anchor(a);
                eprintln!("anchor {a:?}: {ok}");
            }
            if let Some(q) = &self.args.find {
                let st = self.view.set_find_query(q);
                eprintln!("find {q:?}: {st:?}");
            }
            if self.args.select_all {
                self.view.select_all();
                let t = self.view.selected_text().unwrap_or_default();
                eprintln!(
                    "selected {} chars:\n{}",
                    t.len(),
                    t.lines().take(40).collect::<Vec<_>>().join("\n")
                );
            }
        }
        if self.frame == 6
            && let Some(path) = self.args.reload.clone()
        {
            let src = std::fs::read_to_string(&path).expect("read reload file");
            let before = self.view.scroll_offset();
            self.view
                .set_document(Document::parse(&src, path.parent()), true);
            eprintln!("reloaded {}: scroll before {before:.1}", path.display());
        }
        if self.frame == 10 && self.args.reload.is_some() {
            eprintln!("scroll after reload {:.1}", self.view.scroll_offset());
        }
        if self.frame == 10 && self.args.drag.is_some() {
            eprintln!("selection: {:?}", self.view.selected_text());
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.style.palette.bg))
            .show(ui, |ui| {
                let out = self.view.show(ui, &self.style);
                if let Some(link) = out.clicked_link {
                    eprintln!("clicked: {link:?}");
                }
                if out.copied {
                    eprintln!("copied to clipboard");
                }
                if self.frame + 1 == self.args.frames && self.args.screenshot.is_some() {
                    eprintln!(
                        "pointer {:?}, hovered widget at pointer: {:?}",
                        ui.ctx().pointer_hover_pos(),
                        ui.ctx().pointer_hover_pos().map(|p| ui.ctx().layer_id_at(p))
                    );
                    eprintln!(
                        "active heading {:?}, progress {:.3}, words left {}, top line {}, hovered {:?}",
                        out.active_heading,
                        out.progress,
                        out.words_remaining,
                        out.top_source_line,
                        out.hovered_link
                    );
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

/// Headless benchmark: drives an `egui::Context` directly (no window, no GPU), so the numbers
/// are the CPU cost of a frame: `run_ui` (our layout + paint + egui) and tessellation.
fn bench(args: &Args, doc: Document, parse_secs: f64) {
    let ctx = egui::Context::default();
    engine::fonts::install(&ctx);
    if let Some(ppp) = args.ppp {
        ctx.set_pixels_per_point(ppp);
    }
    let mut style = Style::new(args.theme, args.font, args.text_size, args.measure);
    style.wrap_code = args.wrap;
    style.top_inset = args.top_inset;
    let mut view = DocView::new(doc);
    let screen =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(args.size[0], args.size[1]));
    let mut frame = 0u32;
    let mut run = |view: &mut DocView, style: &Style| -> (f64, f64, f64) {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        frame += 1;
        let t0 = std::time::Instant::now();
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(style.palette.bg))
                .show(ui, |ui| {
                    view.show(ui, style);
                });
        });
        let t1 = std::time::Instant::now();
        let mut out = out;
        out.textures_delta.clear(); // no GPU here
        let prims = ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        let t2 = std::time::Instant::now();
        if frame == 200 {
            let verts: usize = prims
                .iter()
                .map(|p| match &p.primitive {
                    egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
                    _ => 0,
                })
                .sum();
            eprintln!(
                "frame {frame}: {} primitives, {verts} vertices",
                prims.len()
            );
        }
        (
            view.last_show_secs(),
            (t1 - t0).as_secs_f64(),
            (t2 - t0).as_secs_f64(),
        )
    };
    let ms = |v: f64| v * 1000.0;
    // Frame 0 builds the font atlas; the document is laid out from frame 1.
    let (_, _, f0) = run(&mut view, &style);
    let (first_show, _, first_frame) = run(&mut view, &style);
    let mut layout_show = first_show;
    let mut layout_frames = 1;
    while !view.layout_complete() && layout_frames < 10_000 {
        let (s, _, _) = run(&mut view, &style);
        layout_show += s;
        layout_frames += 1;
    }
    // Let highlighting finish and settle.
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        run(&mut view, &style);
    }
    let mut show = Vec::new();
    let mut frames = Vec::new();
    let mut y = 0.0f32;
    let mut last = -1.0f32;
    loop {
        y += 37.0;
        view.set_scroll_offset(y);
        let (s, _, f) = run(&mut view, &style);
        show.push(s);
        frames.push(f);
        let off = view.scroll_offset();
        if (off - last).abs() < 0.5 {
            break;
        }
        last = off;
    }
    let stats = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        let avg = v.iter().sum::<f64>() / v.len().max(1) as f64;
        (avg, v[v.len() * 95 / 100], *v.last().unwrap_or(&0.0))
    };
    let (sa, sp, sw) = stats(&mut show);
    let (fa, fp, fw) = stats(&mut frames);
    println!("parse:                         {:.1} ms", ms(parse_secs));
    println!("font atlas (frame 0):          {:.1} ms", ms(f0));
    println!(
        "first frame (visible blocks):  {:.1} ms (show {:.1} ms)",
        ms(first_frame),
        ms(first_show)
    );
    println!(
        "full layout (progressive):     {:.1} ms CPU over {} frames",
        ms(layout_show),
        layout_frames
    );
    println!(
        "parse + first frame:           {:.1} ms",
        ms(parse_secs + first_frame)
    );
    println!(
        "parse + full layout:           {:.1} ms",
        ms(parse_secs + layout_show)
    );
    println!("scrolling, {} frames:", show.len());
    println!(
        "  DocView::show                avg {:.2} ms, p95 {:.2}, worst {:.2}",
        ms(sa),
        ms(sp),
        ms(sw)
    );
    println!(
        "  whole frame (+ tessellation) avg {:.2} ms, p95 {:.2}, worst {:.2}",
        ms(fa),
        ms(fp),
        ms(fw)
    );
}

fn main() -> eframe::Result {
    let args = parse_args();
    let source = std::fs::read_to_string(&args.file).expect("read markdown file");
    let t0 = std::time::Instant::now();
    let doc = Document::parse(&source, args.file.parent());
    let parse_secs = t0.elapsed().as_secs_f64();
    eprintln!(
        "parsed {} bytes in {:?}: {} headings, {} words",
        source.len(),
        t0.elapsed(),
        doc.headings().len(),
        doc.word_count()
    );
    if args.bench {
        bench(&args, doc, parse_secs);
        return Ok(());
    }
    let mut style = Style::new(args.theme, args.font, args.text_size, args.measure);
    style.wrap_code = args.wrap;
    style.top_inset = args.top_inset;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(args.size)
            .with_title("engine preview"),
        ..Default::default()
    };
    eframe::run_native(
        "engine-preview",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            engine::fonts::install(&cc.egui_ctx);
            // SPEC §7: one wheel notch = 3 lines = 78 px.
            cc.egui_ctx
                .options_mut(|o| o.input_options.line_scroll_speed = 78.0);
            if let Some(ppp) = args.ppp {
                cc.egui_ctx.set_pixels_per_point(ppp);
            }
            Ok(Box::new(Preview {
                view: DocView::new(doc),
                style,
                args,
                frame: 0,
            }))
        }),
    )
}
