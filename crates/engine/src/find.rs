//! Find: matches over the rendered text of every run (across inline formatting, in code,
//! tables, alerts, footnotes and collapsed containers). Positions are galley char indices.

use crate::ir::{MARKER, RichText, RunId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub run: RunId,
    /// Galley char range.
    pub start: u32,
    pub end: u32,
}

/// Searchable text of one run: chars without markers, and their galley char indices.
pub struct SearchText {
    chars: Vec<char>,
    folded: Vec<char>,
    map: Vec<u32>,
}

fn fold(c: char) -> char {
    if c == 'ς' {
        return 'σ'; // final sigma
    }
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(a), None) => a,
        _ => c,
    }
}

/// Newlines, tabs and no-break spaces match spaces, in the text and in the query alike (a
/// query pre-filled from a selection keeps them).
fn norm(c: char) -> char {
    if c == '\n' || c == '\t' || c == '\u{A0}' {
        ' '
    } else {
        c
    }
}

impl SearchText {
    pub fn new(rt: &RichText) -> Self {
        let mut chars = Vec::with_capacity(rt.text.len());
        let mut folded = Vec::with_capacity(rt.text.len());
        let mut map = Vec::with_capacity(rt.text.len());
        for (i, c) in rt.text.chars().enumerate() {
            if c == MARKER {
                continue;
            }
            let c = norm(c);
            chars.push(c);
            folded.push(fold(c));
            map.push(i as u32);
        }
        Self { chars, folded, map }
    }
}

/// All non-overlapping matches of `query` in the given runs, in document order.
pub fn find_all(texts: &[SearchText], query: &str, case_sensitive: bool) -> Vec<Match> {
    let q = query.chars().filter(|&c| c != MARKER).map(norm);
    let q: Vec<char> = if case_sensitive {
        q.collect()
    } else {
        q.map(fold).collect()
    };
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    for (run, st) in texts.iter().enumerate() {
        let hay = if case_sensitive {
            &st.chars
        } else {
            &st.folded
        };
        if hay.len() < q.len() {
            continue;
        }
        let mut i = 0;
        while i + q.len() <= hay.len() {
            if hay[i] == q[0] && hay[i..i + q.len()] == q[..] {
                let start = st.map[i];
                let end = st.map[i + q.len() - 1] + 1;
                out.push(Match {
                    run: run as RunId,
                    start,
                    end,
                });
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn count(src: &str, q: &str, cs: bool) -> usize {
        let p = parse(src, None);
        let texts: Vec<SearchText> = p.texts.iter().map(SearchText::new).collect();
        find_all(&texts, q, cs).len()
    }

    #[test]
    fn counts() {
        let src = "# The title\n\nThe **the** t*he* `the` there.\n\n| the | x |\n|---|---|\n| THE | y |\n\n```\nthe code\n```\n\n> [!NOTE]\n> the note\n";
        assert_eq!(count(src, "the", false), 10);
        assert_eq!(count(src, "the", true), 7);
        assert_eq!(count(src, "The", true), 2);
        assert_eq!(count(src, "the code", false), 1);
        assert_eq!(count(src, "xyz", false), 0);
        assert_eq!(count(src, "", false), 0);
    }

    #[test]
    fn queries_from_selections_match_what_is_shown() {
        // A selection keeps NBSPs and tabs; they match the same text find shows.
        let src = "Costs 100&nbsp;USD or 100 USD.\n\n```\nfn\tmain()\n```\n";
        assert_eq!(count(src, "100\u{a0}USD", false), 2);
        assert_eq!(count(src, "100 USD", true), 2);
        assert_eq!(count(src, "fn\tmain", false), 1);
        assert_eq!(count(src, "fn main", false), 1);
        assert_eq!(count("Final ΟΔΟΣ and οδος.", "οδοσ", false), 2);
    }

    #[test]
    fn across_formatting_and_markers() {
        // "use `x` now": the inline code's padding markers are skipped.
        assert_eq!(count("use `x` now", "use x now", false), 1);
        assert_eq!(count("a **bold** word", "a bold word", false), 1);
        // Soft breaks become spaces.
        assert_eq!(count("line one\nline two", "one line", false), 1);
    }

    #[test]
    fn positions_are_galley_chars() {
        let p = parse("a `bc` d", None);
        let texts: Vec<SearchText> = p.texts.iter().map(SearchText::new).collect();
        let m = find_all(&texts, "bc", false);
        // text = "a ⁠bc⁠ d" → 'b' is char 3.
        assert_eq!(
            m,
            vec![Match {
                run: 0,
                start: 3,
                end: 5
            }]
        );
        let m = find_all(&texts, "c d", false);
        assert_eq!(m[0].start, 4);
        assert_eq!(m[0].end, 8);
    }

    #[test]
    fn hero_doc_the() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../samples/ai-report.md"
        ))
        .unwrap();
        let p = parse(&src, None);
        let texts: Vec<SearchText> = p.texts.iter().map(SearchText::new).collect();
        // Every "the" in the rendered text, front matter included (`grep -oi the` agrees: no
        // occurrence sits inside markup or a URL in this document).
        assert_eq!(find_all(&texts, "the", false).len(), 92);
        assert_eq!(find_all(&texts, "The", true).len(), 9);
    }
}
