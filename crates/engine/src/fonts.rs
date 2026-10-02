//! Font registration. The shell calls [`install`] once at startup; the engine owns which
//! families exist and what they're called.

/// Register bundled fonts (and, on Windows, system fallbacks for scripts we don't bundle).
pub fn install(ctx: &egui::Context) {
    // STUB: the engine team bundles the SPEC fonts and registers named families here.
    let _ = ctx;
}
