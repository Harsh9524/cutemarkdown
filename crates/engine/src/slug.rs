//! GitHub-compatible heading slugs (same rules as `github-slugger`).

use std::collections::HashMap;

/// Slug for one heading text, without de-duplication: lowercase, drop everything that isn't a
/// letter, mark, number, connector punctuation (`_`), space or `-`, then spaces become `-`.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        if c == ' ' {
            out.push('-');
        } else if c == '-' || c.is_alphanumeric() || is_connector(c) || is_mark(c) {
            out.push(c);
        }
    }
    out
}

fn is_connector(c: char) -> bool {
    matches!(
        c,
        '_' | '\u{203F}' | '\u{2040}' | '\u{2054}' | '\u{FE33}' | '\u{FE34}' | '\u{FE4D}'
            ..='\u{FE4F}' | '\u{FF3F}'
    )
}

fn is_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{0483}'..='\u{0489}' | '\u{0591}'..='\u{05BD}' | '\u{0610}'..='\u{061A}' | '\u{064B}'..='\u{065F}' | '\u{0900}'..='\u{0903}' | '\u{093A}'..='\u{094F}' | '\u{0951}'..='\u{0957}' | '\u{0962}'..='\u{0963}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
}

/// Hands out unique slugs: the second `usage` becomes `usage-1`, then `usage-2`…
#[derive(Default)]
pub struct Slugger {
    occurrences: HashMap<String, usize>,
}

impl Slugger {
    pub fn slug(&mut self, text: &str) -> String {
        let original = slugify(text);
        let mut result = original.clone();
        while self.occurrences.contains_key(&result) {
            let n = self.occurrences.entry(original.clone()).or_insert(0);
            *n += 1;
            result = format!("{original}-{n}");
        }
        self.occurrences.insert(result.clone(), 0);
        result
    }

    /// Reserve an id that isn't a heading (e.g. an HTML `id`), so later headings avoid it.
    pub fn reserve(&mut self, id: &str) {
        self.occurrences.entry(id.to_owned()).or_insert(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_rules() {
        assert_eq!(slugify("Rollback Plan"), "rollback-plan");
        assert_eq!(
            slugify("Appendix A: Consumer inventory"),
            "appendix-a-consumer-inventory"
        );
        assert_eq!(
            slugify("📋 Background and Motivation"),
            "-background-and-motivation"
        );
        assert_eq!(
            slugify("The render() function and a link"),
            "the-render-function-and-a-link"
        );
        assert_eq!(slugify("11. Blue/Green Deploys"), "11-bluegreen-deploys");
        assert_eq!(slugify("snake_case & more"), "snake_case--more");
        assert_eq!(slugify("Über Café"), "über-café");
        assert_eq!(
            slugify("Phase 0: Preparation (weeks 1-2)"),
            "phase-0-preparation-weeks-1-2"
        );
    }

    #[test]
    fn duplicates() {
        let mut s = Slugger::default();
        assert_eq!(s.slug("Duplicate"), "duplicate");
        assert_eq!(s.slug("Duplicate"), "duplicate-1");
        assert_eq!(s.slug("Duplicate"), "duplicate-2");
        assert_eq!(s.slug("Duplicate 1"), "duplicate-1-1");
        assert_eq!(s.slug("Other"), "other");
    }
}
