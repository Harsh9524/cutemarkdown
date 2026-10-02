//! Syntax highlighting: syntect + two-face grammars, mapped onto our own palette roles
//! (SPEC §5; we never use syntect themes).
//!
//! Highlighting runs on a background worker. Results are cached globally by (language, code)
//! hash, so unchanged blocks are never highlighted twice (e.g. across live reloads). Until a
//! block's result is ready it renders as plain text; [`generation`] changes when new results
//! arrive so views can re-lay out the affected blocks.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use syntect::easy::ScopeRegionIterator;
use syntect::parsing::{
    ParseState, Scope, ScopeStack, SyntaxDefinition, SyntaxReference, SyntaxSet, SyntaxSetBuilder,
};
use syntect::util::LinesWithEndings;

/// Palette role of a code token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Plain,
    Keyword,
    String,
    Number,
    Constant,
    Function,
    Type,
    Comment,
    Operator,
    /// The `-` sign of a deleted diff line (CAUTION fg).
    DelSign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineBg {
    Add,
    Del,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlighted {
    /// Byte ranges into the code with their roles (gaps are plain).
    pub spans: Vec<(Range<usize>, Role)>,
    /// Full-width line backgrounds (diffs): (0-based line, kind).
    pub line_bg: Vec<(u32, LineBg)>,
}

/// Blocks longer than this are not highlighted (SPEC §6).
pub const MAX_LINES: usize = 5_000;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// Our own grammars, for languages two-face only ships for the Oniguruma regex engine.
fn extra_syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut b = SyntaxSetBuilder::new();
        for src in [include_str!("../assets/syntaxes/PowerShell.sublime-syntax")] {
            match SyntaxDefinition::load_from_str(src, true, None) {
                Ok(def) => b.add(def),
                Err(e) => debug_assert!(false, "bundled grammar: {e}"),
            }
        }
        b.build()
    })
}

/// Is `lang` something we can highlight (without loading anything heavy)?
pub fn is_diff(lang: &str) -> bool {
    matches!(lang.to_ascii_lowercase().as_str(), "diff" | "patch")
}

/// The grammar for a fence's language, with the set it belongs to.
fn find_syntax(lang: &str) -> Option<(&'static SyntaxSet, &'static SyntaxReference)> {
    let ss = syntax_set();
    let l = lang.to_ascii_lowercase();
    if matches!(
        l.as_str(),
        "ps1" | "psm1" | "psd1" | "powershell" | "pwsh" | "posh"
    ) {
        let extra = extra_syntax_set();
        return extra.find_syntax_by_name("PowerShell").map(|s| (extra, s));
    }
    let token = match l.as_str() {
        "ts" | "typescript" | "mts" | "cts" => "ts",
        "js" | "javascript" | "mjs" | "cjs" | "node" | "jsx" => "js",
        "sh" | "bash" | "zsh" | "shell" | "console" | "shell-session" | "shellsession" | "ksh" => {
            "bash"
        }
        "yml" | "yaml" => "yaml",
        "json" | "jsonc" | "json5" | "jsonl" => "json",
        "fsharp" | "f#" => "fs",
        "csharp" | "c#" => "cs",
        "c++" => "cpp",
        "golang" => "go",
        "py3" | "python3" => "py",
        "math" | "latex" | "tex" | "katex" => "tex",
        "docker" | "containerfile" => "dockerfile",
        "objc" | "objective-c" | "objectivec" => {
            return ss.find_syntax_by_name("Objective-C").map(|s| (ss, s));
        }
        "text" | "txt" | "plain" | "plaintext" | "" => return None,
        other => other,
    };
    ss.find_syntax_by_token(token)
        .or_else(|| {
            ss.syntaxes()
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(lang))
        })
        .map(|s| (ss, s))
}

fn scopes(list: &[&str]) -> Vec<Scope> {
    list.iter().filter_map(|s| Scope::new(s).ok()).collect()
}

struct Rules {
    rules: Vec<(Vec<Scope>, Role)>,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| Rules {
        // Priority order: the first rule with any matching scope in the stack wins.
        rules: vec![
            (scopes(&["comment"]), Role::Comment),
            (
                scopes(&[
                    "meta.mapping.key",
                    "support.type.property-name",
                    "entity.name.tag.yaml",
                    "meta.tag.key",
                    "entity.name.tag.toml",
                    "meta.object-literal.key",
                    "entity.name.table.toml",
                ]),
                Role::Type,
            ),
            (scopes(&["string", "constant.character"]), Role::String),
            (scopes(&["constant.numeric"]), Role::Number),
            (
                scopes(&[
                    "constant.language",
                    "constant.other",
                    "support.constant",
                    "constant",
                ]),
                Role::Constant,
            ),
            (
                scopes(&[
                    "entity.name.function",
                    "support.function",
                    "variable.function",
                    "meta.annotation.identifier",
                ]),
                Role::Function,
            ),
            (
                scopes(&[
                    "entity.name.type",
                    "entity.name.class",
                    "entity.name.struct",
                    "entity.name.enum",
                    "entity.name.trait",
                    "entity.name.interface",
                    "entity.name.union",
                    "entity.name.namespace",
                    "entity.other.inherited-class",
                    "entity.other.attribute-name",
                    "support.type",
                    "support.class",
                    "variable.annotation",
                ]),
                Role::Type,
            ),
            (scopes(&["keyword.operator"]), Role::Operator),
            (
                scopes(&["keyword", "storage", "entity.name.tag", "variable.language"]),
                Role::Keyword,
            ),
            (scopes(&["punctuation"]), Role::Operator),
        ],
    })
}

fn role_for(stack: &[Scope], cache: &mut HashMap<Vec<Scope>, Role>) -> Role {
    if let Some(r) = cache.get(stack) {
        return *r;
    }
    let mut role = Role::Plain;
    'outer: for (rule_scopes, r) in &rules().rules {
        for s in stack.iter().rev() {
            if rule_scopes.iter().any(|p| p.is_prefix_of(*s)) {
                role = *r;
                break 'outer;
            }
        }
    }
    cache.insert(stack.to_vec(), role);
    role
}

fn push_span(out: &mut Vec<(Range<usize>, Role)>, range: Range<usize>, role: Role) {
    if range.is_empty() || role == Role::Plain {
        return;
    }
    if let Some((last, r)) = out.last_mut()
        && *r == role
        && last.end == range.start
    {
        last.end = range.end;
        return;
    }
    out.push((range, role));
}

/// Highlight synchronously (used by the worker and by tests).
pub fn highlight_now(lang: &str, code: &str) -> Highlighted {
    if code.lines().count() > MAX_LINES {
        return Highlighted::default();
    }
    if is_diff(lang) {
        return highlight_diff(code);
    }
    let Some((ss, syntax)) = find_syntax(lang) else {
        return Highlighted::default();
    };
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut cache = HashMap::new();
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in LinesWithEndings::from(code) {
        let Ok(ops) = state.parse_line(line, ss) else {
            break;
        };
        let mut pos = offset;
        for (text, op) in ScopeRegionIterator::new(&ops, line) {
            if stack.apply(op).is_err() {
                break;
            }
            let end = pos + text.len();
            if !text.trim().is_empty() {
                let role = role_for(stack.as_slice(), &mut cache);
                push_span(&mut out, pos..end, role);
            }
            pos = end;
        }
        offset += line.len();
    }
    Highlighted {
        spans: out,
        line_bg: Vec::new(),
    }
}

fn highlight_diff(code: &str) -> Highlighted {
    let mut h = Highlighted::default();
    let mut offset = 0usize;
    for (i, line) in code.split('\n').enumerate() {
        let len = line.len();
        let r = offset..offset + len;
        if line.starts_with("+++") || line.starts_with("---") {
            push_span(&mut h.spans, r, Role::Keyword);
        } else if line.starts_with("@@") {
            push_span(&mut h.spans, r, Role::Function);
        } else if line.starts_with('+') {
            push_span(&mut h.spans, offset..offset + 1, Role::String);
            h.line_bg.push((i as u32, LineBg::Add));
        } else if line.starts_with('-') {
            push_span(&mut h.spans, offset..offset + 1, Role::DelSign);
            h.line_bg.push((i as u32, LineBg::Del));
        } else if line.starts_with("diff ") || line.starts_with("index ") {
            push_span(&mut h.spans, r, Role::Comment);
        }
        offset += len + 1;
    }
    h
}

// ---------------------------------------------------------------------------------------------
// Background worker + cache

pub type Key = u64;

pub fn key(lang: &str, code: &str) -> Key {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    lang.to_ascii_lowercase().hash(&mut h);
    code.hash(&mut h);
    h.finish()
}

struct Job {
    key: Key,
    lang: String,
    code: String,
}

#[derive(Default)]
struct Shared {
    cache: HashMap<Key, Arc<Highlighted>>,
    queued: HashSet<Key>,
    queue: VecDeque<Job>,
    ctx: Option<egui::Context>,
    started: bool,
}

fn shared() -> &'static (Mutex<Shared>, Condvar) {
    static S: OnceLock<(Mutex<Shared>, Condvar)> = OnceLock::new();
    S.get_or_init(|| (Mutex::new(Shared::default()), Condvar::new()))
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Changes whenever new highlight results are available.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Can this language be highlighted at all? (Cheap: doesn't wait for the worker.)
pub fn supported(lang: &str) -> bool {
    is_diff(lang) || find_syntax(lang).is_some()
}

/// Cached result, or `None` after queueing a job (`urgent` jobs go first).
pub fn get_or_request(
    ctx: &egui::Context,
    lang: &str,
    code: &str,
    urgent: bool,
) -> Option<Arc<Highlighted>> {
    let k = key(lang, code);
    let (lock, cv) = shared();
    let mut s = lock.lock().ok()?;
    if let Some(h) = s.cache.get(&k) {
        return Some(h.clone());
    }
    if s.ctx.is_none() {
        s.ctx = Some(ctx.clone());
    }
    if !s.queued.contains(&k) {
        s.queued.insert(k);
        let job = Job {
            key: k,
            lang: lang.to_owned(),
            code: code.to_owned(),
        };
        if urgent {
            s.queue.push_front(job);
        } else {
            s.queue.push_back(job);
        }
        if !s.started {
            s.started = true;
            std::thread::Builder::new()
                .name("cutemarkdown-highlight".into())
                .spawn(worker)
                .ok();
        }
        cv.notify_one();
    } else if urgent
        && let Some(pos) = s.queue.iter().position(|j| j.key == k)
        && let Some(job) = s.queue.remove(pos)
    {
        s.queue.push_front(job);
    }
    None
}

fn worker() {
    let (lock, cv) = shared();
    // Load grammars up front (cheap: two-face loads lazily).
    let _ = syntax_set();
    loop {
        let job = {
            let Ok(mut s) = lock.lock() else { return };
            loop {
                if let Some(j) = s.queue.pop_front() {
                    break j;
                }
                s = match cv.wait(s) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
        };
        let result = Arc::new(highlight_now(&job.lang, &job.code));
        let ctx = {
            let Ok(mut s) = lock.lock() else { return };
            s.queued.remove(&job.key);
            // Bound the cache (big docs, many reloads).
            if s.cache.len() > 4096 {
                s.cache.clear();
            }
            s.cache.insert(job.key, result);
            GENERATION.fetch_add(1, Ordering::Relaxed);
            if s.queue.is_empty() {
                s.ctx.clone()
            } else {
                None
            }
        };
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_of(h: &Highlighted, code: &str, needle: &str) -> Role {
        let at = code.find(needle).unwrap();
        h.spans
            .iter()
            .find(|(r, _)| r.contains(&at))
            .map(|(_, r)| *r)
            .unwrap_or(Role::Plain)
    }

    #[test]
    fn rust_roles() {
        let code =
            "pub fn handle(x: u32) -> Result<(), E> {\n    // note\n    let s = \"hi\"; 42\n}\n";
        let h = highlight_now("rust", code);
        assert_eq!(role_of(&h, code, "pub"), Role::Keyword);
        assert_eq!(role_of(&h, code, "handle"), Role::Function);
        assert_eq!(role_of(&h, code, "Result"), Role::Type);
        assert_eq!(role_of(&h, code, "// note"), Role::Comment);
        assert_eq!(role_of(&h, code, "\"hi\""), Role::String);
        assert_eq!(role_of(&h, code, "42"), Role::Number);
    }

    #[test]
    fn keys_are_types() {
        let code = "{\"event_id\": \"x\", \"paid\": false}";
        let h = highlight_now("json", code);
        assert_eq!(role_of(&h, code, "event_id"), Role::Type);
        assert_eq!(role_of(&h, code, "\"x\""), Role::String);
        assert_eq!(role_of(&h, code, "false"), Role::Constant);
        let code = "projector:\n  workers: 16\n";
        let h = highlight_now("yml", code);
        assert_eq!(role_of(&h, code, "workers"), Role::Type);
        assert_eq!(role_of(&h, code, "16"), Role::Number);
        let code = "[projector]\nworkers = 16\n";
        let h = highlight_now("toml", code);
        assert_eq!(role_of(&h, code, "workers"), Role::Type);
    }

    #[test]
    fn diff_lines() {
        let code = "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n same";
        let h = highlight_diff(code);
        assert_eq!(h.line_bg, vec![(3, LineBg::Del), (4, LineBg::Add)]);
        assert_eq!(role_of(&h, code, "@@ -1"), Role::Function);
    }

    #[test]
    fn languages_resolve() {
        for l in [
            "ts",
            "sh",
            "bash",
            "yml",
            "toml",
            "dockerfile",
            "json",
            "rust",
            "sql",
            "go",
            "c",
            "java",
            "python",
            "html",
            "css",
            "tex",
        ] {
            assert!(supported(l), "{l}");
        }
        assert!(!supported("mermaid"));
        assert!(!supported("text"));
    }

    #[test]
    fn powershell() {
        for l in ["ps1", "PowerShell", "pwsh", "psm1"] {
            assert!(supported(l), "{l}");
        }
        let code = "# Get the logs\n<# block\n comment #>\nfunction Get-Logs {\n    param([string]$Path = \"$env:TEMP\\logs\")\n    \
                    Get-ChildItem -Path $Path -Filter '*.log' | Where-Object { $_.Length -gt 1KB }\n    \
                    if ($null -eq $x) { return 42 }\n}\n$s = @\"\nHello $name\n\"@\n";
        let h = highlight_now("ps1", code);
        assert_eq!(role_of(&h, code, "Get the"), Role::Comment);
        assert_eq!(role_of(&h, code, "comment"), Role::Comment);
        assert_eq!(role_of(&h, code, "function"), Role::Keyword);
        assert_eq!(role_of(&h, code, "Get-ChildItem"), Role::Function);
        assert_eq!(role_of(&h, code, "string"), Role::Type);
        assert_eq!(role_of(&h, code, "TEMP"), Role::String);
        assert_eq!(role_of(&h, code, "*.log"), Role::String);
        assert_eq!(role_of(&h, code, "-gt"), Role::Operator);
        assert_eq!(role_of(&h, code, "$null"), Role::Constant);
        assert_eq!(role_of(&h, code, "if"), Role::Keyword);
        assert_eq!(role_of(&h, code, "42"), Role::Number);
        assert_eq!(role_of(&h, code, "Hello"), Role::String);
        assert_eq!(role_of(&h, code, "-Filter"), Role::Plain);
    }
}
