//! The status bar under the text.

use crate::app::Tab;
use crate::doc_model::{EditorDoc, format_size};
use crate::layout::display_line;
use editor_core::Eol;
use editor_formats::TextEncoding;
use objc2_foundation::NSString;

pub fn encoding_name(encoding: &TextEncoding) -> String {
    match encoding {
        TextEncoding::Utf8 { bom: false } => "UTF-8".to_string(),
        TextEncoding::Utf8 { bom: true } => "UTF-8 with BOM".to_string(),
        TextEncoding::Utf16Le => "UTF-16 LE".to_string(),
        TextEncoding::Utf16Be => "UTF-16 BE".to_string(),
        TextEncoding::SingleByte(e) => {
            let name = e.name();
            let mut chars = name.chars();
            chars
                .next()
                .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        }
    }
}

pub fn text(doc: &EditorDoc, notice: Option<&str>) -> String {
    let d = &doc.doc;
    let primary = d.selections().primary();
    let (line, column) = d.line_col(primary.head);
    let column = display_line(doc, line).column_of(column);
    let mut parts = vec![format!("Line {}, Column {}", line + 1, column + 1)];
    let selected: usize = d
        .selections()
        .ranges()
        .iter()
        .map(|s| s.max() - s.min())
        .sum();
    if d.selections().ranges().len() > 1 {
        parts.push(format!("{} cursors", d.selections().ranges().len()));
    }
    if selected > 0 {
        parts.push(format!("{selected} selected"));
    }
    parts.push(encoding_name(&doc.encoding));
    parts.push(
        match d.eol() {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
        .to_string(),
    );
    parts.push(doc.language.name().to_string());
    if let Some(container) = &doc.container {
        let total: u64 = container.regions.iter().map(|r| r.len).sum();
        let count = container.regions.len();
        parts.push(format!(
            "FortiWeb backup: {count} protected section{} ({}) locked",
            if count == 1 { "" } else { "s" },
            format_size(total)
        ));
    }
    if let Some(reason) = &doc.read_only {
        parts.push(format!("read-only: {reason}"));
    }
    if let Some(warning) = crate::session_driver::warning() {
        parts.push(warning);
    }
    if let Some(notice) = notice {
        parts.push(notice.to_string());
    }
    parts.join("   \u{b7}   ")
}

pub fn refresh(tab: &Tab) {
    let line = text(&tab.doc.borrow(), tab.notice.borrow().as_deref());
    tab.status.setStringValue(&NSString::from_str(&line));
}
