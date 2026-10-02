//! Font registration. The shell calls [`install`] once at startup; the engine owns which
//! families exist and what they're called.
//!
//! Families (all usable from the shell through [`family`]):
//!
//! * [`egui::FontFamily::Proportional`]: Inter 400 (UI text).
//! * [`egui::FontFamily::Monospace`]: JetBrains Mono NL 400 (no ligatures).
//! * Named families per face and weight, e.g. `family(Face::Sans, 600)` = Inter SemiBold.
//!   Sans and sans italic come in 400, 500, 600, 650 and 700 (Inter is a variable font, so 650
//!   is a real instance). Serif (Literata) comes in 400 and 600.
//!
//! Every family falls back to Noto Emoji (monochrome) and then to system fonts for scripts we
//! don't bundle (CJK, Arabic, Hebrew, Indic, Thai…). System fonts are loaded lazily, on a
//! background thread, only when a document actually contains such characters.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily, FontTweak};

pub(crate) static INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Variable.ttf");
pub(crate) static INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-Italic-Variable.ttf");
pub(crate) static LITERATA: &[u8] = include_bytes!("../assets/fonts/Literata-Regular.ttf");
pub(crate) static LITERATA_SEMIBOLD: &[u8] =
    include_bytes!("../assets/fonts/Literata-SemiBold.ttf");
pub(crate) static LITERATA_ITALIC: &[u8] = include_bytes!("../assets/fonts/Literata-Italic.ttf");
pub(crate) static LITERATA_SEMIBOLD_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/Literata-SemiBoldItalic.ttf");
pub(crate) static JETBRAINS_MONO: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf");
pub(crate) static NOTO_EMOJI: &[u8] = include_bytes!("../assets/fonts/NotoEmoji-Regular.ttf");

/// Typeface roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Face {
    /// Inter.
    Sans,
    /// Inter Italic.
    SansItalic,
    /// Literata.
    Serif,
    /// Literata Italic.
    SerifItalic,
    /// JetBrains Mono NL.
    Mono,
}

/// Weights available for [`Face::Sans`] and [`Face::SansItalic`].
pub const SANS_WEIGHTS: [u16; 5] = [400, 500, 600, 650, 700];
/// Weights available for [`Face::Serif`] and [`Face::SerifItalic`].
pub const SERIF_WEIGHTS: [u16; 2] = [400, 600];

fn nearest_weight(face: Face, weight: u16) -> u16 {
    let list: &[u16] = match face {
        Face::Sans | Face::SansItalic => &SANS_WEIGHTS,
        Face::Serif | Face::SerifItalic => &SERIF_WEIGHTS,
        Face::Mono => &[400],
    };
    *list
        .iter()
        .min_by_key(|w| (i32::from(**w) - i32::from(weight)).unsigned_abs())
        .unwrap_or(&400)
}

fn family_name(face: Face, weight: u16) -> String {
    let base = match face {
        Face::Sans => "Inter",
        Face::SansItalic => "Inter Italic",
        Face::Serif => "Literata",
        Face::SerifItalic => "Literata Italic",
        Face::Mono => "JetBrains Mono",
    };
    format!("{base} {weight}")
}

/// The egui font family for a face at (the nearest available) weight.
///
/// `family(Face::Sans, 400)` is [`FontFamily::Proportional`] and `family(Face::Mono, _)` is
/// [`FontFamily::Monospace`]; everything else is a named family registered by [`install`].
pub fn family(face: Face, weight: u16) -> FontFamily {
    static CACHE: OnceLock<HashMap<(Face, u16), FontFamily>> = OnceLock::new();
    let weight = nearest_weight(face, weight);
    if face == Face::Mono {
        return FontFamily::Monospace;
    }
    if face == Face::Sans && weight == 400 {
        return FontFamily::Proportional;
    }
    let cache = CACHE.get_or_init(|| {
        let mut m = HashMap::new();
        for face in [Face::Sans, Face::SansItalic] {
            for w in SANS_WEIGHTS {
                m.insert((face, w), FontFamily::Name(family_name(face, w).into()));
            }
        }
        for face in [Face::Serif, Face::SerifItalic] {
            for w in SERIF_WEIGHTS {
                m.insert((face, w), FontFamily::Name(family_name(face, w).into()));
            }
        }
        m
    });
    cache
        .get(&(face, weight))
        .cloned()
        .unwrap_or(FontFamily::Proportional)
}

/// Inter Medium (500), for UI labels.
pub fn ui_medium() -> FontFamily {
    family(Face::Sans, 500)
}

/// Inter SemiBold (600), for UI emphasis (e.g. the "Aa" button).
pub fn ui_semibold() -> FontFamily {
    family(Face::Sans, 600)
}

/// Inter Bold (700).
pub fn ui_bold() -> FontFamily {
    family(Face::Sans, 700)
}

/// Literata Regular (e.g. the "Serif" label in the Aa popover).
pub fn serif() -> FontFamily {
    family(Face::Serif, 400)
}

/// Incremented whenever the set of installed fonts changes (layout caches key on it).
static FONT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn generation() -> u64 {
    FONT_GENERATION.load(Ordering::Relaxed)
}

const EMOJI_KEY: &str = "Noto Emoji";

fn emoji_data() -> FontData {
    // Noto Emoji glyphs are drawn large (1.27em advance); bring them in line with Inter.
    FontData::from_static(NOTO_EMOJI).tweak(FontTweak {
        scale: 0.86,
        ..Default::default()
    })
}

fn variable(bytes: &'static [u8], weight: u16) -> FontData {
    FontData::from_static(bytes).tweak(FontTweak {
        coords: VariationCoords::new([(b"wght", f32::from(weight))]),
        ..Default::default()
    })
}

/// Every family name we register, with its primary fonts (before the emoji + system fallbacks).
fn all_families() -> Vec<(FontFamily, Vec<String>)> {
    let mut out = Vec::new();
    for w in SANS_WEIGHTS {
        let primary = vec![format!("inter-{w}")];
        if w == 400 {
            out.push((FontFamily::Proportional, primary.clone()));
        }
        out.push((family_name_family(Face::Sans, w), primary));
        out.push((
            family_name_family(Face::SansItalic, w),
            vec![format!("inter-italic-{w}"), format!("inter-{w}")],
        ));
    }
    for w in SERIF_WEIGHTS {
        // Literata lacks arrows and many symbols: fall back to Inter at a matching weight.
        let inter_w = if w >= 600 { 600 } else { 400 };
        out.push((
            family_name_family(Face::Serif, w),
            vec![format!("literata-{w}"), format!("inter-{inter_w}")],
        ));
        out.push((
            family_name_family(Face::SerifItalic, w),
            vec![
                format!("literata-italic-{w}"),
                format!("inter-italic-{inter_w}"),
            ],
        ));
    }
    out.push((
        FontFamily::Monospace,
        vec!["jetbrains-mono".to_owned(), "inter-400".to_owned()],
    ));
    out
}

fn family_name_family(face: Face, w: u16) -> FontFamily {
    FontFamily::Name(family_name(face, w).into())
}

/// Register bundled fonts (and, on Windows, system fallbacks for scripts we don't bundle).
///
/// Builds the font definitions from scratch: egui's default fonts are not used.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::empty();
    for w in SANS_WEIGHTS {
        defs.font_data
            .insert(format!("inter-{w}"), Arc::new(variable(INTER, w)));
        defs.font_data.insert(
            format!("inter-italic-{w}"),
            Arc::new(variable(INTER_ITALIC, w)),
        );
    }
    defs.font_data.insert(
        "literata-400".into(),
        Arc::new(FontData::from_static(LITERATA)),
    );
    defs.font_data.insert(
        "literata-600".into(),
        Arc::new(FontData::from_static(LITERATA_SEMIBOLD)),
    );
    defs.font_data.insert(
        "literata-italic-400".into(),
        Arc::new(FontData::from_static(LITERATA_ITALIC)),
    );
    defs.font_data.insert(
        "literata-italic-600".into(),
        Arc::new(FontData::from_static(LITERATA_SEMIBOLD_ITALIC)),
    );
    defs.font_data.insert(
        "jetbrains-mono".into(),
        Arc::new(FontData::from_static(JETBRAINS_MONO)),
    );
    defs.font_data
        .insert(EMOJI_KEY.into(), Arc::new(emoji_data()));

    // System fallbacks that were already loaded (e.g. `install` called again) stay installed.
    let loaded: Vec<(String, Arc<FontData>)> = fallback_state()
        .lock()
        .map(|s| s.installed.clone())
        .unwrap_or_default();
    for (name, data) in &loaded {
        defs.font_data.insert(name.clone(), data.clone());
    }

    for (fam, mut list) in all_families() {
        list.push(EMOJI_KEY.to_owned());
        list.extend(loaded.iter().map(|(n, _)| n.clone()));
        defs.families.insert(fam, list);
    }
    ctx.set_fonts(defs);
    FONT_GENERATION.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------------------------
// Coverage of the bundled fonts.

fn bundled_charmaps() -> &'static [skrifa::FontRef<'static>] {
    static REFS: OnceLock<Vec<skrifa::FontRef<'static>>> = OnceLock::new();
    REFS.get_or_init(|| {
        [INTER, INTER_ITALIC, JETBRAINS_MONO, NOTO_EMOJI]
            .into_iter()
            .filter_map(|b| skrifa::FontRef::new(b).ok())
            .collect()
    })
}

/// Characters that never need a glyph (format/control characters, variation selectors).
fn ignorable(c: char) -> bool {
    c.is_control()
        || c.is_whitespace()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}')
        || ('\u{E0000}'..='\u{E007F}').contains(&c)
        || ('\u{1F3FB}'..='\u{1F3FF}').contains(&c) // skin tone modifiers
}

pub(crate) fn bundled_covers(c: char) -> bool {
    use skrifa::MetadataProvider as _;
    if (c as u32) < 0x80 || ignorable(c) {
        return true;
    }
    bundled_charmaps()
        .iter()
        .any(|f| f.charmap().map(c).is_some())
}

/// Characters of `chars` that no bundled font has.
pub(crate) fn missing_chars(chars: impl IntoIterator<Item = char>) -> Vec<char> {
    chars.into_iter().filter(|&c| !bundled_covers(c)).collect()
}

// ---------------------------------------------------------------------------------------------
// System fallback fonts.

#[derive(Default)]
struct FallbackState {
    /// Characters we already looked for (found or not).
    requested: BTreeSet<char>,
    /// Font files already considered (path, face index).
    used_files: BTreeSet<(String, u32)>,
    /// Loaded by the worker, not yet given to egui.
    ready: Vec<(String, Arc<FontData>)>,
    /// Given to egui (kept so a second `install` keeps them).
    installed: Vec<(String, Arc<FontData>)>,
    busy: bool,
    /// Characters requested while the worker was busy.
    queued: BTreeSet<char>,
}

fn fallback_state() -> &'static Mutex<FallbackState> {
    static STATE: OnceLock<Mutex<FallbackState>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

/// One candidate system font: path and, for `.ttc` collections, preferred family names.
struct Candidate {
    path: std::path::PathBuf,
    prefer: &'static [&'static str],
}

fn candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    if cfg!(windows) {
        let root = std::env::var_os("WINDIR")
            .or_else(|| std::env::var_os("SystemRoot"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        // SPEC §4 order, plus Leelawadee UI (Thai, Lao, Khmer) before the symbol font.
        let list: &[(&str, &[&str])] = &[
            ("segoeui.ttf", &[]),
            ("msyh.ttc", &["Microsoft YaHei UI"]),
            ("YuGothM.ttc", &["Yu Gothic UI"]),
            ("YuGothR.ttc", &["Yu Gothic UI"]),
            ("malgun.ttf", &[]),
            ("Nirmala.ttc", &["Nirmala UI"]),
            ("Nirmala.ttf", &[]),
            ("LeelawUI.ttf", &[]),
            ("seguisym.ttf", &[]),
            ("seguihis.ttf", &[]),
        ];
        for (file, prefer) in list {
            out.push(Candidate {
                path: root.join(file),
                prefer,
            });
        }
    } else {
        let list: &[(&str, &[&str])] = &[
            ("/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", &[]),
            (
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                &["Noto Sans CJK SC"],
            ),
            (
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                &["Noto Sans CJK SC"],
            ),
            (
                "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
                &[],
            ),
            (
                "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
                &[],
            ),
            (
                "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
                &[],
            ),
            (
                "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf",
                &[],
            ),
            ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", &[]),
            (
                "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
                &["WenQuanYi Zen Hei"],
            ),
            (
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                &["WenQuanYi Micro Hei"],
            ),
            ("/usr/share/fonts/truetype/freefont/FreeSerif.ttf", &[]),
            ("/usr/share/fonts/truetype/freefont/FreeSans.ttf", &[]),
            ("/System/Library/Fonts/PingFang.ttc", &[]),
            ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", &[]),
        ];
        for (file, prefer) in list {
            out.push(Candidate {
                path: file.into(),
                prefer,
            });
        }
    }
    out
}

/// Pick the face index of a collection: a preferred family name if present, else the first
/// face covering any wanted character.
fn pick_face(bytes: &[u8], prefer: &[&str], wanted: &[char]) -> Option<(u32, Vec<char>)> {
    use skrifa::MetadataProvider as _;
    let count = match skrifa::raw::FileRef::new(bytes).ok()? {
        skrifa::raw::FileRef::Font(_) => 1,
        skrifa::raw::FileRef::Collection(c) => c.len(),
    };
    let mut best: Option<(u32, Vec<char>)> = None;
    for index in 0..count {
        let Ok(font) = skrifa::FontRef::from_index(bytes, index) else {
            continue;
        };
        let cmap = font.charmap();
        let covered: Vec<char> = wanted
            .iter()
            .copied()
            .filter(|&c| cmap.map(c).is_some())
            .collect();
        if covered.is_empty() {
            continue;
        }
        let name: String = font
            .localized_strings(skrifa::string::StringId::FAMILY_NAME)
            .english_or_first()
            .map(|s| s.chars().collect())
            .unwrap_or_default();
        let preferred = prefer.iter().any(|p| name.eq_ignore_ascii_case(p));
        if preferred {
            return Some((index, covered));
        }
        if best.as_ref().is_none_or(|(_, c)| covered.len() > c.len()) {
            best = Some((index, covered));
        }
    }
    best
}

fn load_fallbacks_blocking(
    wanted: Vec<char>,
    used: BTreeSet<(String, u32)>,
) -> Vec<(String, (String, u32), FontData)> {
    let mut remaining = wanted;
    let mut out = Vec::new();
    for cand in candidates() {
        if remaining.is_empty() {
            break;
        }
        let key_path = cand.path.to_string_lossy().into_owned();
        if used.iter().any(|(p, _)| *p == key_path) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&cand.path) else {
            continue;
        };
        let Some((index, covered)) = pick_face(&bytes, cand.prefer, &remaining) else {
            continue;
        };
        remaining.retain(|c| !covered.contains(c));
        let name = format!(
            "system:{}#{index}",
            cand.path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        out.push((name, (key_path, index), data));
    }
    out
}

/// Ask for system fonts covering `chars` (characters no bundled font has). Returns immediately;
/// loading happens on a background thread and [`poll_fallbacks`] installs the result.
pub(crate) fn request_fallbacks(ctx: &egui::Context, chars: &[char]) {
    let Ok(mut st) = fallback_state().lock() else {
        return;
    };
    let new: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| !st.requested.contains(c))
        .collect();
    if new.is_empty() {
        return;
    }
    if st.busy {
        st.queued.extend(new);
        return;
    }
    st.requested.extend(new.iter().copied());
    st.busy = true;
    let used = st.used_files.clone();
    drop(st);
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("cutemarkdown-fallback-fonts".into())
        .spawn(move || {
            let fonts = load_fallbacks_blocking(new, used);
            if let Ok(mut st) = fallback_state().lock() {
                for (name, file, data) in fonts {
                    st.used_files.insert(file);
                    st.ready.push((name, Arc::new(data)));
                }
                st.busy = false;
            }
            ctx.request_repaint();
        })
        .ok();
}

/// Install any system fallback fonts that finished loading. Returns `true` if fonts changed
/// (they take effect next frame; [`generation`] is bumped).
pub(crate) fn poll_fallbacks(ctx: &egui::Context) -> bool {
    let (ready, queued) = {
        let Ok(mut st) = fallback_state().lock() else {
            return false;
        };
        if st.busy {
            return false;
        }
        let queued: Vec<char> = std::mem::take(&mut st.queued).into_iter().collect();
        (std::mem::take(&mut st.ready), queued)
    };
    if !queued.is_empty() {
        request_fallbacks(ctx, &queued);
    }
    if ready.is_empty() {
        return false;
    }
    for (name, data) in &ready {
        let families = all_families()
            .into_iter()
            .map(|(family, _)| InsertFontFamily {
                family,
                priority: FontPriority::Lowest,
            })
            .collect();
        ctx.add_font(FontInsert::new(name, (**data).clone(), families));
    }
    if let Ok(mut st) = fallback_state().lock() {
        st.installed.extend(ready);
    }
    FONT_GENERATION.fetch_add(1, Ordering::Relaxed);
    ctx.request_repaint();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_coverage() {
        for c in "Hello, wörld — “quotes” → ↗ ↩ ✓ ✅ ❌ ⚠ 🟢 🔴 🚀 Ωμέγα Привет".chars()
        {
            assert!(bundled_covers(c), "{c:?} should be covered");
        }
        assert!(!bundled_covers('你'));
        assert!(!bundled_covers('ع'));
        assert!(bundled_covers('\u{FE0F}'));
    }

    #[test]
    fn inter_has_ui_arrows() {
        use skrifa::MetadataProvider as _;
        let inter = skrifa::FontRef::new(INTER).unwrap();
        for c in "←→↑↓↗↩⌘⇧…·×✓".chars() {
            assert!(inter.charmap().map(c).is_some(), "Inter lacks {c:?}");
        }
    }

    #[test]
    fn ui_families() {
        assert_eq!(ui_medium(), FontFamily::Name("Inter 500".into()));
        assert_eq!(ui_semibold(), FontFamily::Name("Inter 600".into()));
        assert_eq!(ui_bold(), FontFamily::Name("Inter 700".into()));
        assert_eq!(serif(), FontFamily::Name("Literata 400".into()));
    }

    #[test]
    fn weights_snap() {
        assert_eq!(family(Face::Sans, 400), FontFamily::Proportional);
        assert_eq!(family(Face::Mono, 700), FontFamily::Monospace);
        assert_eq!(
            family(Face::Sans, 640),
            FontFamily::Name("Inter 650".into())
        );
        assert_eq!(
            family(Face::Serif, 650),
            FontFamily::Name("Literata 600".into())
        );
    }
}
