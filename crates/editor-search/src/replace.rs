//! Building replacement text.

use crate::engine::{Cancelled, SearchText, find_all};
use crate::query::Query;
use editor_core::{Edit, EditBatch};
use std::ops::Range;
use std::sync::atomic::AtomicBool;

/// Expands `$1`, `${1}`, `${name}`, `$$`, and `\n`, `\t`, `\r`, `\\` in regex mode.
pub fn expand_replacement(
    query: &Query,
    t: &SearchText,
    found: &Range<usize>,
    template: &str,
) -> String {
    if !query.is_regex() {
        return template.to_string();
    }
    let groups = query.groups_at(&t.text, t.byte_of(found.start));
    let group = |index: usize| -> &str {
        groups
            .get(index)
            .cloned()
            .flatten()
            .map_or("", |range| &t.text[range])
    };
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        match c {
            '\\' => {
                let Some(next) = rest.chars().next() else {
                    out.push('\\');
                    break;
                };
                let mapped = match next {
                    'n' => Some('\n'),
                    't' => Some('\t'),
                    'r' => Some('\r'),
                    '\\' => Some('\\'),
                    _ => None,
                };
                match mapped {
                    Some(m) => {
                        out.push(m);
                        rest = &rest[next.len_utf8()..];
                    }
                    None => out.push('\\'),
                }
            }
            '$' if rest.starts_with('$') => {
                out.push('$');
                rest = &rest[1..];
            }
            '$' if rest.starts_with('{') => match rest.find('}') {
                Some(close) => {
                    let name = &rest[1..close];
                    let index = name.parse().ok().or_else(|| query.group_index(name));
                    out.push_str(index.map_or("", group));
                    rest = &rest[close + 1..];
                }
                None => out.push('$'),
            },
            '$' if rest.starts_with(|d: char| d.is_ascii_digit()) => {
                let digits = rest.bytes().take_while(u8::is_ascii_digit).count().min(2);
                let two = (digits == 2)
                    .then(|| rest[..2].parse::<usize>().ok())
                    .flatten()
                    .filter(|&n| n < groups.len());
                let (index, used) = match two {
                    Some(n) => (n, 2),
                    None => (rest[..1].parse().unwrap_or(0), 1),
                };
                out.push_str(group(index));
                rest = &rest[used..];
            }
            other => out.push(other),
        }
    }
    out
}

/// Prepares every replacement as one batch of edits.
pub fn replace_all(
    query: &Query,
    t: &SearchText,
    template: &str,
    scope: Option<Range<usize>>,
    cancel: &AtomicBool,
) -> Result<EditBatch, Cancelled> {
    let matches = find_all(query, t, scope, cancel)?;
    Ok(EditBatch(
        matches
            .into_iter()
            .map(|range| {
                let text = expand_replacement(query, t, &range, template);
                Edit { range, text }
            })
            .collect(),
    ))
}
