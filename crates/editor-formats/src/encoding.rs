//! Text encodings that keep every untouched byte.

use editor_core::Eol;
use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, WINDOWS_1252};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const EOL_SAMPLE: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8 { bom: bool },
    Utf16Le,
    Utf16Be,
    SingleByte(&'static Encoding),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Editable,
    ReadOnly { reason: String },
}

/// A character the file's encoding cannot store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnrepresentableChar {
    pub ch: char,
    pub offset: usize,
}

/// The bytes do not survive a round trip through the requested encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingMismatch;

fn utf16(bytes: &[u8], encoding: &'static Encoding) -> Option<String> {
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|t| t.into_owned())
}

/// Decodes a file, falling back to a one-byte-per-character mapping.
pub fn decode_bytes(bytes: &[u8]) -> (String, TextEncoding, OpenMode) {
    let decoded = if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        utf16(rest, UTF_16LE).map(|t| (t, TextEncoding::Utf16Le))
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        utf16(rest, UTF_16BE).map(|t| (t, TextEncoding::Utf16Be))
    } else {
        None
    };
    if let Some((text, encoding)) = decoded {
        return (text, encoding, OpenMode::Editable);
    }
    let mode = if memchr::memchr(0, bytes).is_some() {
        OpenMode::ReadOnly {
            reason: "This file contains binary data, so it opens read-only.".to_string(),
        }
    } else {
        OpenMode::Editable
    };
    let (body, bom) = match bytes.strip_prefix(UTF8_BOM) {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    if let Ok(text) = std::str::from_utf8(body) {
        return (text.to_string(), TextEncoding::Utf8 { bom }, mode);
    }
    let text = WINDOWS_1252
        .decode_without_bom_handling(bytes)
        .0
        .into_owned();
    (text, TextEncoding::SingleByte(WINDOWS_1252), mode)
}

/// Encodes text for saving. Never substitutes a character silently.
pub fn encode_text(text: &str, encoding: &TextEncoding) -> Result<Vec<u8>, UnrepresentableChar> {
    match *encoding {
        TextEncoding::Utf8 { bom } => {
            let mut out = Vec::with_capacity(text.len() + 3);
            if bom {
                out.extend_from_slice(UTF8_BOM);
            }
            out.extend_from_slice(text.as_bytes());
            Ok(out)
        }
        TextEncoding::Utf16Le => {
            let mut out = b"\xFF\xFE".to_vec();
            text.encode_utf16()
                .for_each(|u| out.extend_from_slice(&u.to_le_bytes()));
            Ok(out)
        }
        TextEncoding::Utf16Be => {
            let mut out = b"\xFE\xFF".to_vec();
            text.encode_utf16()
                .for_each(|u| out.extend_from_slice(&u.to_be_bytes()));
            Ok(out)
        }
        TextEncoding::SingleByte(single) => {
            let mut out = Vec::with_capacity(text.len());
            let mut buffer = [0u8; 4];
            for (offset, ch) in text.chars().enumerate() {
                let (bytes, _, unmappable) = single.encode(ch.encode_utf8(&mut buffer));
                if unmappable {
                    return Err(UnrepresentableChar { ch, offset });
                }
                out.extend_from_slice(&bytes);
            }
            Ok(out)
        }
    }
}

/// Decodes with a chosen encoding, refusing any that would alter the file.
pub fn reopen_with(bytes: &[u8], encoding: &'static Encoding) -> Result<String, EncodingMismatch> {
    let text = encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or(EncodingMismatch)?;
    let (encoded, _, unmappable) = encoding.encode(&text);
    if unmappable || encoded.as_ref() != bytes {
        return Err(EncodingMismatch);
    }
    Ok(text.into_owned())
}

/// Reopens bytes with an encoding chosen from the menu.
/// Refuses any choice that would not save back exactly the same bytes.
pub fn reopen_as(bytes: &[u8], name: &str) -> Result<(String, TextEncoding), EncodingMismatch> {
    let (text, encoding) = match name {
        "UTF-8" => {
            let (body, bom) = match bytes.strip_prefix(UTF8_BOM) {
                Some(rest) => (rest, true),
                None => (bytes, false),
            };
            (
                std::str::from_utf8(body)
                    .map_err(|_| EncodingMismatch)?
                    .to_string(),
                TextEncoding::Utf8 { bom },
            )
        }
        "UTF-16 LE" => {
            let body = bytes.strip_prefix(b"\xFF\xFE").ok_or(EncodingMismatch)?;
            (
                utf16(body, UTF_16LE).ok_or(EncodingMismatch)?,
                TextEncoding::Utf16Le,
            )
        }
        "UTF-16 BE" => {
            let body = bytes.strip_prefix(b"\xFE\xFF").ok_or(EncodingMismatch)?;
            (
                utf16(body, UTF_16BE).ok_or(EncodingMismatch)?,
                TextEncoding::Utf16Be,
            )
        }
        label => {
            let encoding = Encoding::for_label(label.as_bytes()).ok_or(EncodingMismatch)?;
            (
                reopen_with(bytes, encoding)?,
                TextEncoding::SingleByte(encoding),
            )
        }
    };
    if encode_text(&text, &encoding).ok().as_deref() != Some(bytes) {
        return Err(EncodingMismatch);
    }
    Ok((text, encoding))
}

/// The most common line ending, judged from the start of the text.
pub fn dominant_eol(text: &str) -> Eol {
    let bytes = &text.as_bytes()[..text.len().min(EOL_SAMPLE)];
    let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
    for at in memchr::memchr2_iter(b'\n', b'\r', bytes) {
        match bytes[at] {
            b'\n' if at > 0 && bytes[at - 1] == b'\r' => crlf += 1,
            b'\n' => lf += 1,
            _ if bytes.get(at + 1) == Some(&b'\n') => {}
            _ => cr += 1,
        }
    }
    if crlf > lf && crlf >= cr {
        Eol::CrLf
    } else if cr > lf {
        Eol::Cr
    } else {
        Eol::Lf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::Eol;
    use proptest::prelude::*;

    #[test]
    fn all_256_bytes_round_trip_as_windows_1252() {
        let bytes: Vec<u8> = (1..=255).collect();
        let (t, e, m) = decode_bytes(&bytes);
        assert_eq!(e, TextEncoding::SingleByte(encoding_rs::WINDOWS_1252));
        assert_eq!(m, OpenMode::Editable);
        assert_eq!(encode_text(&t, &e).unwrap(), bytes);
    }

    #[test]
    fn utf8_bom_is_preserved() {
        let bytes = b"\xEF\xBB\xBFhello";
        let (t, e, _) = decode_bytes(bytes);
        assert_eq!(t, "hello");
        assert_eq!(e, TextEncoding::Utf8 { bom: true });
        assert_eq!(encode_text(&t, &e).unwrap(), bytes);
    }

    #[test]
    fn utf16_le_and_be_round_trip_with_bom() {
        for (bytes, expected) in [
            (&b"\xFF\xFEh\0i\0"[..], TextEncoding::Utf16Le),
            (&b"\xFE\xFF\0h\0i"[..], TextEncoding::Utf16Be),
        ] {
            let (t, e, m) = decode_bytes(bytes);
            assert_eq!(t, "hi");
            assert_eq!(e, expected);
            assert_eq!(m, OpenMode::Editable);
            assert_eq!(encode_text(&t, &e).unwrap(), bytes);
        }
    }

    #[test]
    fn nul_in_unrecognized_file_opens_read_only() {
        assert!(matches!(
            decode_bytes(b"ab\0c").2,
            OpenMode::ReadOnly { .. }
        ));
        assert!(matches!(
            decode_bytes(b"\xFFab\0c").2,
            OpenMode::ReadOnly { .. }
        ));
    }

    #[test]
    fn invalid_utf16_falls_back_to_single_byte() {
        let (_, e, m) = decode_bytes(b"\xFF\xFEh");
        assert_eq!(e, TextEncoding::SingleByte(encoding_rs::WINDOWS_1252));
        assert_eq!(m, OpenMode::Editable);
    }

    #[test]
    fn emoji_in_1252_reports_unrepresentable() {
        let e = TextEncoding::SingleByte(encoding_rs::WINDOWS_1252);
        let err = encode_text("ab😀", &e).unwrap_err();
        assert_eq!(
            err,
            UnrepresentableChar {
                ch: '😀',
                offset: 2
            }
        );
    }

    #[test]
    fn reopen_accepts_a_faithful_encoding() {
        assert_eq!(
            reopen_with(b"caf\xE9", encoding_rs::ISO_8859_2).unwrap(),
            "café"
        );
    }

    #[test]
    fn reopen_refuses_a_lossy_encoding() {
        assert!(reopen_with(b"\xA1", encoding_rs::ISO_8859_8).is_err());
    }

    #[test]
    fn reopen_as_utf16_without_bom_is_refused() {
        assert!(reopen_as(b"h\0i\0", "UTF-16 LE").is_err());
    }

    #[test]
    fn reopen_as_utf16_with_matching_bom_round_trips() {
        let (text, encoding) = reopen_as(b"\xFF\xFEh\0i\0", "UTF-16 LE").unwrap();
        assert_eq!(text, "hi");
        assert_eq!(encode_text(&text, &encoding).unwrap(), b"\xFF\xFEh\0i\0");
    }

    #[test]
    fn reopen_as_wrong_byte_order_is_refused() {
        assert!(reopen_as(b"\xFF\xFEh\0i\0", "UTF-16 BE").is_err());
    }

    #[test]
    fn reopen_as_utf8_keeps_a_bom() {
        let (text, encoding) = reopen_as(b"\xEF\xBB\xBFhi", "UTF-8").unwrap();
        assert_eq!(text, "hi");
        assert_eq!(encode_text(&text, &encoding).unwrap(), b"\xEF\xBB\xBFhi");
    }

    #[test]
    fn reopen_as_single_byte_round_trips() {
        let (text, _) = reopen_as(b"caf\xE9", "ISO-8859-2").unwrap();
        assert_eq!(text, "café");
    }

    #[test]
    fn dominant_eol_counts_each_kind() {
        assert_eq!(dominant_eol("a\r\nb\r\nc\n"), Eol::CrLf);
        assert_eq!(dominant_eol("a\rb\rc\n"), Eol::Cr);
        assert_eq!(dominant_eol("plain"), Eol::Lf);
    }

    #[test]
    fn edge_case_files_round_trip() {
        for b in [
            &b""[..],
            b"no newline",
            b"\r\r\r",
            b"a\r\nb\nc\rd",
            b"\n",
            b"\xEF\xBB\xBF",
        ] {
            let (t, e, _) = decode_bytes(b);
            assert_eq!(encode_text(&t, &e).unwrap(), b);
        }
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_round_trip(bytes in proptest::collection::vec(1u8..=255, 0..400)) {
            let (t, e, _) = decode_bytes(&bytes);
            prop_assert_eq!(encode_text(&t, &e).unwrap(), bytes);
        }
    }
}
