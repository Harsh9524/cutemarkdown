//! cutemarkdown: a fast, minimal, native Markdown reader.
//!
//! `main` parses the command line, loads settings, opens extra files in their own processes,
//! creates the window and, if the GPU renderer can't start, re-launches with the next backend.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cli;
mod decode;
mod history;
mod icons;
mod links;
mod platform;
mod reload;
mod renderer;
mod settings;
mod theme;
mod ui;

use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use eframe::egui;

use crate::renderer::Renderer;
use crate::settings::SettingsStore;

fn main() -> ExitCode {
    let args = match cli::parse(std::env::args_os().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cutemarkdown: {e}\n\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!("{}", cli::USAGE);
        return ExitCode::SUCCESS;
    }

    let mut settings = load_settings(&args);
    if args.is_qa() {
        settings.set_writable(false);
    }

    let attempt = renderer::current_attempt();
    // Files after the first open in their own windows (only on the first launch, not on renderer
    // retries, and not in screenshot runs).
    if attempt == 0 && args.screenshot.is_none() && !args.empty {
        for file in args.files.iter().skip(1) {
            let mut child_args = Vec::new();
            if let Some(s) = &args.settings {
                child_args.extend(["--settings".into(), s.clone().into_os_string()]);
            }
            child_args.extend(["--".into(), file.clone().into_os_string()]);
            if let Err(e) = platform::spawn_window(&child_args) {
                eprintln!("cutemarkdown: couldn't open {}: {e}", file.display());
            }
        }
    }

    let preferred = settings.data.renderer;
    let Some(renderer) = renderer::for_attempt(preferred, attempt) else {
        return renderer_gave_up("no backend left to try");
    };
    let wait_for_retry = args.screenshot.is_some();
    let options = native_options(&args, &settings, renderer);
    let result = eframe::run_native(
        "cutemarkdown",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, args, settings, renderer)))),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // Failed before showing anything, and not in our own app code: most likely the GPU
        // backend. Try the next one in a fresh process (winit can't re-create its event loop).
        Err(e)
            if !app::FIRST_FRAME_SHOWN.load(Ordering::Relaxed)
                && !matches!(e, eframe::Error::AppCreation(_)) =>
        {
            relaunch_with_next_renderer(preferred, attempt, &e.to_string(), wait_for_retry)
        }
        Err(e) => {
            platform::fatal_message(
                "cutemarkdown",
                &format!("cutemarkdown stopped unexpectedly.\n\n{e}"),
            );
            ExitCode::FAILURE
        }
    }
}

fn load_settings(args: &cli::Args) -> SettingsStore {
    let Some(path) = args.settings.clone().or_else(settings::default_path) else {
        return SettingsStore::memory(Default::default());
    };
    let (store, issue) = SettingsStore::load(path);
    if let Some(issue) = issue {
        eprintln!("cutemarkdown: settings: {issue:?}; using defaults");
    }
    store
}

fn native_options(
    args: &cli::Args,
    settings: &SettingsStore,
    renderer: Renderer,
) -> eframe::NativeOptions {
    let title = match args.files.first() {
        Some(f) if !args.empty => format!("{} — cutemarkdown", ui::menu::file_name(f)),
        _ => "cutemarkdown".to_owned(),
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("cutemarkdown")
        .with_min_inner_size([480.0, 360.0])
        .with_drag_and_drop(true);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/brand/png/logo-256.png"))
    {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let mut centered = true;
    if args.size.is_some() || args.ppp.is_some() {
        // QA: --size is in points at --ppp, so a 1.5× shot shows the same layout as 1.0×.
        let [w, h] = args.size.unwrap_or([1100.0, 860.0]);
        let ppp = args.ppp.unwrap_or(1.0);
        viewport = viewport.with_inner_size([w * ppp, h * ppp]);
    } else if let Some(g) = settings.data.window.filter(|_| !args.is_qa()) {
        viewport = viewport
            .with_inner_size([g.w, g.h])
            .with_maximized(g.maximized);
        // Windows whose saved spot is off every monitor are re-centered.
        if platform::rect_on_screen(g.x, g.y, g.w, g.h) {
            viewport = viewport.with_position([g.x, g.y]);
            centered = false;
        }
    } else {
        viewport = viewport.with_inner_size([1100.0, 860.0]);
    }

    let mut options = eframe::NativeOptions {
        viewport,
        centered,
        // Geometry lives in our settings JSON; eframe's own persistence stays unused. Pointing
        // it next to our settings keeps eframe from creating a separate data folder.
        persist_window: false,
        persistence_path: settings.path().map(|p| p.with_file_name("eframe.ron")),
        ..Default::default()
    };
    if let Some(backends) = renderer_backends(renderer)
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
    {
        setup.instance_descriptor.backends = backends;
    }
    options
}

/// wgpu backends for a renderer choice (`None` = wgpu's default, which honors `WGPU_BACKEND`).
fn renderer_backends(renderer: Renderer) -> Option<eframe::wgpu::Backends> {
    match renderer {
        Renderer::Auto => None,
        Renderer::Vulkan => Some(eframe::wgpu::Backends::VULKAN),
        Renderer::Gl => Some(eframe::wgpu::Backends::GL),
    }
}

/// Re-launch this exe with the same arguments and the next renderer, then exit.
fn relaunch_with_next_renderer(
    preferred: Renderer,
    attempt: usize,
    error: &str,
    wait: bool,
) -> ExitCode {
    let next_attempt = attempt + 1;
    let Some(next) = renderer::for_attempt(preferred, next_attempt) else {
        return renderer_gave_up(error);
    };
    eprintln!("cutemarkdown: renderer failed to start ({error}); retrying with {next:?}");
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return renderer_gave_up(&format!("{error}; can't find own exe: {e}")),
    };
    let mut cmd = Command::new(exe);
    cmd.args(std::env::args_os().skip(1))
        .env(renderer::ATTEMPT_ENV, next_attempt.to_string());
    match next.wgpu_backend() {
        Some(b) => cmd.env(renderer::WGPU_BACKEND_ENV, b),
        None => cmd.env_remove(renderer::WGPU_BACKEND_ENV),
    };
    match cmd.spawn() {
        // Screenshot runs wait so scripts see the real result; normal launches just hand over.
        Ok(mut child) if wait => match child.wait() {
            Ok(status) if status.success() => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        },
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => renderer_gave_up(&format!("{error}; couldn't re-launch: {e}")),
    }
}

fn renderer_gave_up(details: &str) -> ExitCode {
    platform::fatal_message(
        "cutemarkdown",
        &format!(
            "cutemarkdown couldn't start its renderer. It tried DirectX 12, Vulkan and OpenGL.\n\n\
             Updating your graphics driver usually fixes this.\n\nDetails: {details}"
        ),
    );
    ExitCode::FAILURE
}
