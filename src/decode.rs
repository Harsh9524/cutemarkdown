//! Turning file bytes into text (SPEC §7 "Encoding").
//!
//! UTF-8 (with or without BOM) and UTF-16 with BOM are decoded exactly. Bytes that aren't valid
//! UTF-8 are either mostly-UTF-8 with a few bad bytes (decoded lossily) or legacy 8-bit text,
//! which on Windows is almost always Windows-1252.

/// Decode a Markdown file's bytes.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    // Invalid UTF-8. If real multi-byte characters outnumber the broken sequences, it's UTF-8
    // with a few bad bytes; otherwise treat it as Windows-1252.
    let (mut valid_multibyte, mut invalid) = (0usize, 0usize);
    for chunk in bytes.utf8_chunks() {
        valid_multibyte += chunk.valid().chars().filter(|c| !c.is_ascii()).count();
        invalid += usize::from(!chunk.invalid().is_empty());
    }
    if valid_multibyte > invalid {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        decode_windows_1252(bytes)
    }
}

fn decode_utf16(bytes: &[u8], to_unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.chunks_exact(2).map(|c| to_unit([c[0], c[1]]));
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Windows-1252: Latin-1 except 0x80–0x9F, which hold typographic punctuation. Undefined
/// positions map to the C1 control of the same value (as WHATWG does).
fn decode_windows_1252(bytes: &[u8]) -> String {
    const HIGH: [u16; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x008D, 0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
        0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => {
                char::from_u32(u32::from(HIGH[usize::from(b - 0x80)])).unwrap_or('\u{FFFD}')
            }
            _ => char::from(b),
        })
        .collect()
}

/// Stable 64-bit FNV-1a hash, used to ignore reloads whose content didn't change.
pub fn content_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_plain_and_bom() {
        assert_eq!(decode("# Héllo ✅".as_bytes()), "# Héllo ✅");
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("# Hi".as_bytes());
        assert_eq!(decode(&b), "# Hi");
    }

    #[test]
    fn utf16_le_and_be_with_bom() {
        let text = "# Ünïcode 🚀";
        let mut le = vec![0xFF, 0xFE];
        let mut be = vec![0xFE, 0xFF];
        for u in text.encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode(&le), text);
        assert_eq!(decode(&be), text);
    }

    #[test]
    fn legacy_bytes_fall_back_to_windows_1252() {
        // "café – “quoted” €5" in Windows-1252.
        let bytes = b"caf\xE9 \x96 \x93quoted\x94 \x805";
        assert_eq!(decode(bytes), "café – “quoted” €5");
    }

    #[test]
    fn mostly_utf8_with_a_bad_byte_stays_utf8() {
        let mut bytes = "naïve résumé – ok ".as_bytes().to_vec();
        bytes.push(0xFF);
        let s = decode(&bytes);
        assert!(s.starts_with("naïve résumé – ok "));
        assert!(s.ends_with('\u{FFFD}'));
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        assert_eq!(content_hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash("abc"), content_hash("abc"));
        assert_ne!(content_hash("abc"), content_hash("abd"));
    }
}
