//! Incremental highlighting with tree-sitter grammars.

use crate::detect::Language;
use crate::role::Role;
use ropey::Rope;
use std::ops::Range;
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, Tree};

fn grammar(language: Language) -> Option<(tree_sitter::Language, &'static str)> {
    Some(match language {
        Language::Json => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
        ),
        Language::Shell => (
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
        ),
        Language::Python => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
        ),
        Language::Html => (
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
        ),
        Language::Css => (
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
        ),
        Language::JavaScript => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
        ),
        Language::Php => (
            tree_sitter_php::LANGUAGE_PHP.into(),
            tree_sitter_php::HIGHLIGHTS_QUERY,
        ),
        Language::Plain | Language::FortiWebCli | Language::Diff => return None,
    })
}

fn role_for(capture: &str) -> Role {
    let mut parts = capture.split('.');
    let head = parts.next().unwrap_or_default();
    let tail = parts.next();
    match (head, tail) {
        ("comment", _) => Role::Comment,
        ("string", Some("escape")) | ("escape", _) => Role::StringEscape,
        ("string", _) => Role::String,
        ("number" | "float" | "boolean" | "constant", _) => Role::Number,
        ("keyword" | "conditional" | "repeat" | "include" | "exception", _) => Role::Keyword,
        ("variable", Some("builtin")) => Role::Keyword,
        ("function" | "method", _) => Role::Function,
        ("type" | "constructor", _) => Role::Type,
        ("variable", _) => Role::Variable,
        ("property" | "field" | "label", _) => Role::Property,
        ("operator", _) => Role::Operator,
        ("tag", _) => Role::Tag,
        ("attribute", _) => Role::Attribute,
        _ => Role::Plain,
    }
}

struct Grammar {
    parser: Parser,
    query: Query,
    tree: Option<Tree>,
}

impl Grammar {
    fn new(language: Language) -> Option<Self> {
        let (grammar, source) = grammar(language)?;
        let mut parser = Parser::new();
        parser.set_language(&grammar).ok()?;
        let query = Query::new(&grammar, source).ok()?;
        Some(Self {
            parser,
            query,
            tree: None,
        })
    }

    fn parse(&mut self, rope: &Rope, incremental: bool) {
        let len = rope.len_bytes();
        let mut read = |byte: usize, _: Point| -> &[u8] {
            if byte >= len {
                return &[];
            }
            let (chunk, start, _, _) = rope.chunk_at_byte(byte);
            &chunk.as_bytes()[byte - start..]
        };
        let old = if incremental {
            self.tree.as_ref()
        } else {
            None
        };
        self.tree = self.parser.parse_with_options(&mut read, old, None);
    }

    fn spans(&self, rope: &Rope, range: Range<usize>, out: &mut Vec<(Range<usize>, Role)>) {
        let Some(tree) = &self.tree else { return };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let text = |node: Node| {
            rope.byte_slice(node.byte_range())
                .chunks()
                .map(str::as_bytes)
        };
        let mut captures = cursor.captures(&self.query, tree.root_node(), text);
        let names = self.query.capture_names();
        while let Some((found, index)) = captures.next() {
            let capture = found.captures()[*index];
            let role = role_for(names[capture.index as usize]);
            if role != Role::Plain {
                out.push((capture.node.byte_range(), role));
            }
        }
    }
}

/// Highlights one document, re-parsing only what changed.
pub struct TreeHighlighter {
    main: Grammar,
    embedded: Vec<(&'static str, Grammar)>,
}

impl TreeHighlighter {
    pub fn new(language: Language) -> Option<Self> {
        let main = Grammar::new(language)?;
        let embedded = if language == Language::Html {
            vec![
                ("style_element", Grammar::new(Language::Css)?),
                ("script_element", Grammar::new(Language::JavaScript)?),
            ]
        } else {
            Vec::new()
        };
        Some(Self { main, embedded })
    }

    /// Tells the tree about edits before the next `parse`.
    pub fn edit(&mut self, edits: &[InputEdit]) {
        if let Some(tree) = &mut self.main.tree {
            edits.iter().for_each(|edit| tree.edit(edit));
        }
    }

    pub fn parse(&mut self, rope: &Rope) {
        self.main.parse(rope, true);
        let Some(tree) = &self.main.tree else { return };
        for (element, grammar) in &mut self.embedded {
            let mut ranges = Vec::new();
            collect_raw_text(tree.root_node(), element, &mut ranges);
            if ranges.is_empty() || grammar.parser.set_included_ranges(&ranges).is_err() {
                grammar.tree = None;
                continue;
            }
            grammar.parse(rope, false);
        }
    }

    /// Colored byte ranges overlapping `byte_range`, later entries win.
    pub fn spans(&self, rope: &Rope, byte_range: Range<usize>) -> Vec<(Range<usize>, Role)> {
        let mut out = Vec::new();
        self.main.spans(rope, byte_range.clone(), &mut out);
        for (_, grammar) in &self.embedded {
            grammar.spans(rope, byte_range.clone(), &mut out);
        }
        out
    }
}

fn collect_raw_text(node: Node, element: &str, ranges: &mut Vec<tree_sitter::Range>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == element {
            let mut inner = child.walk();
            ranges.extend(
                child
                    .children(&mut inner)
                    .filter(|c| c.kind() == "raw_text")
                    .map(|c| c.range()),
            );
        } else {
            collect_raw_text(child, element, ranges);
        }
    }
}

fn point_at(rope: &Rope, byte: usize) -> Point {
    let row = rope.byte_to_line(byte);
    Point {
        row,
        column: byte - rope.line_to_byte(row),
    }
}

/// Describes replacing `chars` with `text`. Call before changing the rope.
pub fn input_edit(rope: &Rope, chars: Range<usize>, text: &str) -> InputEdit {
    let start_byte = rope.char_to_byte(chars.start);
    let old_end_byte = rope.char_to_byte(chars.end);
    let start_position = point_at(rope, start_byte);
    let new_end_position = match text.rfind('\n') {
        Some(last) => Point {
            row: start_position.row + text.matches('\n').count(),
            column: text.len() - last - 1,
        },
        None => Point {
            row: start_position.row,
            column: start_position.column + text.len(),
        },
    };
    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte: start_byte + text.len(),
        start_position,
        old_end_position: point_at(rope, old_end_byte),
        new_end_position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn role_of(language: Language, source: &str, word: &str) -> Role {
        let rope = Rope::from_str(source);
        let mut h = TreeHighlighter::new(language).expect("grammar");
        h.parse(&rope);
        let at = source.find(word).unwrap();
        h.spans(&rope, 0..rope.len_bytes())
            .iter()
            .rev()
            .find(|(r, _)| r.start <= at && at < r.end)
            .map_or(Role::Plain, |(_, role)| *role)
    }

    #[test]
    fn each_language_highlights_a_keyword_or_string() {
        for (language, source, word, role) in [
            (Language::Json, "{\"a\": 12}", "12", Role::Number),
            (Language::Shell, "echo \"hi\"", "\"hi\"", Role::String),
            (
                Language::Python,
                "def f():\n    return 1\n",
                "def",
                Role::Keyword,
            ),
            (Language::Css, "a { color: red; }", "color", Role::Property),
            (Language::JavaScript, "const x = 1;", "const", Role::Keyword),
            (Language::Php, "<?php echo 'x'; ?>", "echo", Role::Keyword),
            (Language::Html, "<div class=\"a\"></div>", "div", Role::Tag),
        ] {
            assert_eq!(role_of(language, source, word), role, "{language:?}");
        }
    }

    #[test]
    fn plain_languages_have_no_tree() {
        assert!(TreeHighlighter::new(Language::Plain).is_none());
        assert!(TreeHighlighter::new(Language::FortiWebCli).is_none());
        assert!(TreeHighlighter::new(Language::Diff).is_none());
    }

    #[test]
    fn html_colors_embedded_css_and_javascript() {
        let source = "<style>a { color: red; }</style><script>const x = 1;</script>";
        assert_eq!(role_of(Language::Html, source, "color"), Role::Property);
        assert_eq!(role_of(Language::Html, source, "const"), Role::Keyword);
    }

    #[test]
    fn incremental_edit_matches_full_parse() {
        let mut rope = Rope::from_str("x = 1\nprint(x)\n");
        let mut h = TreeHighlighter::new(Language::Python).unwrap();
        h.parse(&rope);
        let edit = input_edit(&rope, 6..6, "def g():\n    pass\n");
        h.edit(&[edit]);
        rope.insert(6, "def g():\n    pass\n");
        h.parse(&rope);
        let mut fresh = TreeHighlighter::new(Language::Python).unwrap();
        fresh.parse(&rope);
        assert_eq!(
            h.spans(&rope, 0..rope.len_bytes()),
            fresh.spans(&rope, 0..rope.len_bytes())
        );
    }

    #[test]
    fn spans_respect_the_requested_range() {
        let rope = Rope::from_str("a = 1\nb = 2\n");
        let mut h = TreeHighlighter::new(Language::Python).unwrap();
        h.parse(&rope);
        assert!(h.spans(&rope, 6..12).iter().all(|(r, _)| r.end > 6));
    }
}
