//! Checks how real Fortinet configuration files are colored and folded.
//! Prints only counts, line numbers and command keywords, never values.

use editor_formats::load_file;
use editor_syntax::{Language, LineState, cli_structure, detect, highlight_cli_line};
use std::collections::BTreeMap;
use std::path::Path;

fn keyword_like(word: &str) -> bool {
    word.len() <= 24
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn check(path: &Path) -> Result<(), String> {
    let file = load_file(path).map_err(|e| e.to_string())?;
    let head: String = file.text.lines().take(50).collect::<Vec<_>>().join("\n");
    let language = detect(Some(path), &head);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
    let lines: Vec<&str> = file.text.lines().collect();
    println!(
        "{name}: {:?}, {:?}, {} lines",
        file.kind,
        language,
        lines.len()
    );
    if language != Language::FortiWebCli {
        println!("  NOT detected as Fortinet CLI");
    }
    let structure = cli_structure(lines.iter().copied());
    println!(
        "  folds: {}, block issues: {}",
        structure.folds.len(),
        structure.issues.len()
    );
    for issue in structure.issues.iter().take(10) {
        println!("    line {}: {}", issue.line + 1, issue.message);
    }
    let mut state = LineState::default();
    let mut roles: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown_lines = Vec::new();
    let mut locked = 0usize;
    for (n, line) in lines.iter().enumerate() {
        let at_statement = state == LineState::default();
        let (spans, next) = highlight_cli_line(line, &state);
        for span in &spans {
            *roles.entry(format!("{:?}", span.role)).or_default() += 1;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('\u{FFFC}') {
            locked += 1;
            state = next;
            continue;
        }
        if at_statement
            && !trimmed.is_empty()
            && !spans
                .iter()
                .any(|s| s.range.start == line.len() - line.trim_start().len())
        {
            let word = trimmed.split_whitespace().next().unwrap_or("");
            let shown = if keyword_like(word) {
                word.to_string()
            } else {
                "<other>".to_string()
            };
            *unknown.entry(shown).or_default() += 1;
            if unknown_lines.len() < 8 {
                unknown_lines.push(n + 1);
            }
        }
        state = next;
    }
    if locked > 0 {
        println!("  locked protected sections: {locked}");
    }
    if state != LineState::default() && !file.text.trim_end().ends_with("####") {
        println!("  file ends inside a quoted string");
    }
    println!(
        "  uncolored statement starts: {} (first at lines {:?})",
        unknown.values().sum::<usize>(),
        unknown_lines
    );
    for (word, count) in &unknown {
        println!("    {word}: {count}");
    }
    let total: usize = roles.values().sum();
    println!(
        "  colored spans: {total}; {}",
        roles
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(())
}

fn main() {
    let mut failed = false;
    for arg in std::env::args().skip(1) {
        if let Err(e) = check(Path::new(&arg)) {
            println!("{arg}: {e}");
            failed = true;
        }
    }
    std::process::exit(i32::from(failed));
}
