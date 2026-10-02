//! A small, forgiving HTML tokenizer for the safe subset in SPEC §6.
//!
//! It never executes or fetches anything: it only splits raw HTML into tags and text so the
//! parser can map the allowed tags onto the IR and drop everything else.

/// One HTML token.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// Raw text (entities already decoded).
    Text(String),
    Start {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    End {
        name: String,
    },
    /// Comments, doctypes, processing instructions: hidden.
    Ignored,
}

impl Token {
    pub fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Elements dropped together with their content.
pub fn is_dangerous(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "iframe"
            | "object"
            | "embed"
            | "template"
            | "noscript"
            | "form"
            | "input"
            | "button"
            | "select"
            | "option"
            | "optgroup"
            | "textarea"
            | "video"
            | "audio"
            | "canvas"
            | "applet"
            | "frame"
            | "frameset"
            | "head"
            | "title"
            | "meta"
            | "link"
            | "base"
            | "math"
    )
}

/// Void elements (never have a closing tag).
pub fn is_void(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr"
            | "img"
            | "input"
            | "meta"
            | "link"
            | "source"
            | "wbr"
            | "area"
            | "base"
            | "col"
            | "embed"
            | "param"
            | "track"
    )
}

/// Elements whose content is raw text (no tags inside).
fn is_raw_text(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "textarea" | "title" | "xmp" | "iframe" | "noscript" | "noembed"
    )
}

pub fn tokenize(src: &str) -> Vec<Token> {
    tokenize_spans(src).into_iter().map(|(t, _)| t).collect()
}

/// Tokenize, keeping each token's byte range in `src`.
pub fn tokenize_spans(src: &str) -> Vec<(Token, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let flush = |out: &mut Vec<(Token, std::ops::Range<usize>)>, from: usize, to: usize| {
        if to > from {
            out.push((Token::Text(decode_entities(&src[from..to])), from..to));
        }
    };
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &src[i..];
        if let Some(after) = rest.strip_prefix("<!--") {
            flush(&mut out, text_start, i);
            let end = after
                .find("-->")
                .map(|e| i + 4 + e + 3)
                .unwrap_or(bytes.len());
            out.push((Token::Ignored, i..end));
            i = end;
            text_start = i;
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            flush(&mut out, text_start, i);
            let end = rest.find('>').map(|e| i + e + 1).unwrap_or(bytes.len());
            out.push((Token::Ignored, i..end));
            i = end;
            text_start = i;
        } else if let Some((tok, len)) = parse_tag(rest) {
            flush(&mut out, text_start, i);
            let tag_start = i;
            i += len;
            // Raw-text elements: swallow everything up to the matching end tag.
            if let Token::Start {
                name,
                self_closing: false,
                ..
            } = &tok
                && is_raw_text(name)
            {
                let close = format!("</{name}");
                let lower = src[i..].to_ascii_lowercase();
                let end = lower.find(&close).map(|e| i + e).unwrap_or(bytes.len());
                let name = name.clone();
                out.push((tok, tag_start..i));
                if end > i {
                    out.push((Token::Text(src[i..end].to_owned()), i..end));
                }
                i = end;
                if i < bytes.len() {
                    let gt = src[i..].find('>').map(|e| i + e + 1).unwrap_or(bytes.len());
                    out.push((Token::End { name }, i..gt));
                    i = gt;
                }
                text_start = i;
                continue;
            }
            out.push((tok, tag_start..i));
            text_start = i;
        } else {
            i += 1;
        }
    }
    flush(&mut out, text_start, bytes.len());
    out
}

/// Parse a start or end tag at the beginning of `s`. Returns the token and its byte length.
fn parse_tag(s: &str) -> Option<(Token, usize)> {
    let b = s.as_bytes();
    let mut i = 1;
    let end_tag = b.get(1) == Some(&b'/');
    if end_tag {
        i += 1;
    }
    if !b.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let name_start = i;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
        i += 1;
    }
    let name = s[name_start..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return None; // unterminated tag: treat as text
        }
        match b[i] {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                self_closing = true;
                i += 1;
                continue;
            }
            b'<' => return None,
            _ => {}
        }
        // attribute name
        let an_start = i;
        while i < b.len()
            && !b[i].is_ascii_whitespace()
            && !matches!(b[i], b'=' | b'>' | b'/' | b'<')
        {
            i += 1;
        }
        if i == an_start {
            i += 1;
            continue;
        }
        let an = s[an_start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    let vs = i + 1;
                    let ve = s[vs..].find(q as char).map(|e| vs + e)?;
                    value = decode_entities(&s[vs..ve]);
                    i = ve + 1;
                }
                Some(_) => {
                    let vs = i;
                    while i < b.len()
                        && !b[i].is_ascii_whitespace()
                        && b[i] != b'>'
                        && !(b[i] == b'/' && b.get(i + 1) == Some(&b'>'))
                    {
                        i += 1;
                    }
                    value = decode_entities(&s[vs..i]);
                }
                None => return None,
            }
        }
        attrs.push((an, value));
    }
    let tok = if end_tag {
        Token::End { name }
    } else {
        Token::Start {
            name,
            attrs,
            self_closing,
        }
    };
    Some((tok, i))
}

/// Decode HTML character references (named subset, decimal and hex).
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let semi = rest[1..].find(';').map(|e| e + 1).filter(|&e| e <= 33);
        if let Some(semi) = semi {
            let ent = &rest[1..semi];
            if let Some(c) = decode_one(ent) {
                out.push(c);
                rest = &rest[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn decode_one(ent: &str) -> Option<char> {
    if let Some(num) = ent.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        return Some(
            char::from_u32(code)
                .filter(|&c| c != '\0')
                .unwrap_or('\u{FFFD}'),
        );
    }
    Some(match ent {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{A0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "laquo" => '«',
        "raquo" => '»',
        "lsaquo" => '‹',
        "rsaquo" => '›',
        "ldquo" => '“',
        "rdquo" => '”',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "bdquo" => '„',
        "bull" => '•',
        "middot" => '·',
        "times" => '×',
        "divide" => '÷',
        "deg" => '°',
        "plusmn" => '±',
        "para" => '¶',
        "sect" => '§',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "lArr" => '⇐',
        "rArr" => '⇒',
        "hearts" => '♥',
        "check" => '✓',
        "star" => '☆',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwj" => '\u{200D}',
        "zwnj" => '\u{200C}',
        "shy" => '\u{AD}',
        "iexcl" => '¡',
        "iquest" => '¿',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "micro" => 'µ',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "infin" => '∞',
        "minus" => '−',
        "prime" => '′',
        "Prime" => '″',
        "alpha" => 'α',
        "beta" => 'β',
        "gamma" => 'γ',
        "delta" => 'δ',
        "pi" => 'π',
        "sigma" => 'σ',
        "lambda" => 'λ',
        "mu" => 'μ',
        "Omega" => 'Ω',
        "omega" => 'ω',
        "auml" => 'ä',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "Auml" => 'Ä',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "szlig" => 'ß',
        "eacute" => 'é',
        "egrave" => 'è',
        "aacute" => 'á',
        "agrave" => 'à',
        "ccedil" => 'ç',
        "ntilde" => 'ñ',
        _ => return None,
    })
}

/// Collapse HTML whitespace runs to single spaces.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
            ws = true;
        } else {
            if ws {
                out.push(' ');
            }
            ws = false;
            out.push(c);
        }
    }
    if ws {
        out.push(' ');
    }
    out
}

/// Is this URL safe to follow or load? Rejects `javascript:`, `vbscript:` and `data:` (except
/// raster `data:image/…` for images, which we don't support either, so: rejected).
pub fn is_safe_url(url: &str) -> bool {
    let t: String = url
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    let lower = t.to_ascii_lowercase();
    !(lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_text() {
        let t = tokenize("<kbd>Ctrl</kbd> &amp; <img src=\"a.png\" alt='x y' width=20/>");
        assert_eq!(
            t[0],
            Token::Start {
                name: "kbd".into(),
                attrs: vec![],
                self_closing: false
            }
        );
        assert_eq!(t[1], Token::Text("Ctrl".into()));
        assert_eq!(t[2], Token::End { name: "kbd".into() });
        assert_eq!(t[3], Token::Text(" & ".into()));
        match &t[4] {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                assert_eq!(name, "img");
                assert!(self_closing);
                assert_eq!(Token::attr(attrs, "alt"), Some("x y"));
                assert_eq!(Token::attr(attrs, "width"), Some("20"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn script_is_raw() {
        let t = tokenize("<script>if (a < b) { alert('<x>') }</script>after");
        assert_eq!(t.len(), 4);
        assert!(matches!(&t[1], Token::Text(s) if s.contains("alert")));
        assert_eq!(
            t[2],
            Token::End {
                name: "script".into()
            }
        );
        assert_eq!(t[3], Token::Text("after".into()));
    }

    #[test]
    fn comments_and_entities() {
        let t = tokenize("a<!-- hidden <b> -->b &#x1F680; &copy; &unknown;");
        assert_eq!(t[0], Token::Text("a".into()));
        assert_eq!(t[1], Token::Ignored);
        assert_eq!(t[2], Token::Text("b 🚀 © &unknown;".into()));
    }

    #[test]
    fn unsafe_urls() {
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url(" JavaScript:alert(1)"));
        assert!(!is_safe_url("java\tscript:alert(1)"));
        assert!(is_safe_url("https://example.com"));
        assert!(is_safe_url("./a.md#x"));
    }
}
