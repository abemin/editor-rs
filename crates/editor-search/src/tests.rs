use super::*;
use ropey::Rope;
use std::sync::atomic::AtomicBool;

fn regex() -> SearchOptions {
    SearchOptions {
        regex: true,
        ..Default::default()
    }
}
fn text(s: &str) -> SearchText {
    SearchText::new(&Rope::from_str(s), &[])
}
fn all(p: &str, o: &SearchOptions, s: &str) -> Vec<std::ops::Range<usize>> {
    find_all(
        &Query::compile(p, o).unwrap(),
        &text(s),
        None,
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn expand(p: &str, s: &str, template: &str) -> String {
    let q = Query::compile(p, &regex()).unwrap();
    let t = text(s);
    let m = find_next(&q, &t, 0, false, false, None).unwrap();
    expand_replacement(&q, &t, &m, template)
}

#[test]
fn literal_metacharacters_are_literal() {
    assert!(all("a.b", &SearchOptions::default(), "axb").is_empty());
    assert_eq!(all("a.b", &SearchOptions::default(), "a.b"), vec![0..3]);
}

#[test]
fn caret_and_dollar_work_per_line_with_crlf() {
    assert_eq!(all("^b$", &regex(), "a\r\nb\r\nc"), vec![3..4]);
}

#[test]
fn multiline_pattern_matches() {
    assert_eq!(all("a\\nb", &regex(), "xa\nby"), vec![1..4]);
}

#[test]
fn lookaround_and_backreference_use_fallback() {
    assert_eq!(all("(?<=x)y", &regex(), "ay xy"), vec![4..5]);
    assert_eq!(all("(a)\\1", &regex(), "ab aa"), vec![3..5]);
}

#[test]
fn invalid_pattern_has_message() {
    let error = Query::compile("(", &regex()).err().unwrap();
    assert!(!error.message.is_empty());
}

#[test]
fn empty_pattern_is_rejected() {
    assert!(Query::compile("", &SearchOptions::default()).is_err());
}

#[test]
fn whole_word_and_ignore_case() {
    let o = SearchOptions {
        whole_word: true,
        ignore_case: true,
        ..Default::default()
    };
    assert_eq!(all("set", &o, "reset SET settle"), vec![6..9]);
}

#[test]
fn replacement_groups() {
    assert_eq!(
        expand("set ip (\\d+)", "set ip 10", "ip=$1a ${1}b $$"),
        "ip=10a 10b $"
    );
}

#[test]
fn named_group_replacement() {
    assert_eq!(expand("(?P<n>\\d+)", "42", "<${n}>"), "<42>");
}

#[test]
fn replacement_escapes_expand_in_regex_mode() {
    assert_eq!(expand("x", "x", "a\\nb\\t\\\\"), "a\nb\t\\");
}

#[test]
fn literal_mode_replacement_is_verbatim() {
    let q = Query::compile("x", &SearchOptions::default()).unwrap();
    let t = text("x");
    assert_eq!(expand_replacement(&q, &t, &(0..1), "$1\\n"), "$1\\n");
}

#[test]
fn group_that_did_not_take_part_is_empty() {
    assert_eq!(expand("(a)|(b)", "b", "[$1][$2]"), "[][b]");
}

#[test]
fn offsets_are_chars_not_bytes() {
    assert_eq!(all("x", &SearchOptions::default(), "é€x"), vec![2..3]);
    let long = "€".repeat(5000) + "x";
    assert_eq!(all("x", &SearchOptions::default(), &long), vec![5000..5001]);
}

#[test]
fn matches_overlapping_locked_placeholder_are_skipped() {
    let t = SearchText::new(&Rope::from_str("ab\u{FFFC}cd ab"), &[2]);
    let q = Query::compile("b.c", &regex()).unwrap();
    assert!(
        find_all(&q, &t, None, &AtomicBool::new(false))
            .unwrap()
            .is_empty()
    );
    let q = Query::compile("ab", &regex()).unwrap();
    assert_eq!(
        find_all(&q, &t, None, &AtomicBool::new(false))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn find_next_wraps_and_goes_backward() {
    let q = Query::compile("a", &SearchOptions::default()).unwrap();
    let t = text("a b a");
    assert_eq!(find_next(&q, &t, 1, false, false, None), Some(4..5));
    assert_eq!(find_next(&q, &t, 5, false, false, None), None);
    assert_eq!(find_next(&q, &t, 5, false, true, None), Some(0..1));
    assert_eq!(find_next(&q, &t, 4, true, false, None), Some(0..1));
    assert_eq!(find_next(&q, &t, 0, true, true, None), Some(4..5));
}

#[test]
fn replace_all_in_selection_only() {
    let q = Query::compile("a", &SearchOptions::default()).unwrap();
    let t = text("a a a");
    let batch = replace_all(&q, &t, "b", Some(1..4), &AtomicBool::new(false)).unwrap();
    assert_eq!(batch.0.len(), 1);
    assert_eq!(batch.0[0].range, 2..3);
}

#[test]
fn replace_all_expands_groups_in_order() {
    let q = Query::compile("(\\d)", &regex()).unwrap();
    let t = text("1 2");
    let batch = replace_all(&q, &t, "<$1>", None, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        batch.0.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
        ["<1>", "<2>"]
    );
}

#[test]
fn cancel_stops_find_all() {
    let q = Query::compile("a", &SearchOptions::default()).unwrap();
    assert!(find_all(&q, &text("aaa"), None, &AtomicBool::new(true)).is_err());
}

#[test]
fn empty_matches_do_not_loop() {
    assert_eq!(all("x*", &regex(), "ab").len(), 3);
    assert_eq!(all("^", &regex(), "a\nb").len(), 2);
}
