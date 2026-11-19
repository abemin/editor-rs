//! Compiling what the user typed into a search query.

use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub ignore_case: bool,
    pub whole_word: bool,
    pub wrap: bool,
}

/// Shown next to the find field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    pub message: String,
}

enum Engine {
    Fast(regex::Regex),
    Backtracking(fancy_regex::Regex),
}

pub struct Query {
    engine: Engine,
    is_regex: bool,
    names: HashMap<String, usize>,
}

/// Byte ranges of each capture group.
pub(crate) type Groups = Vec<Option<Range<usize>>>;

impl Query {
    pub fn compile(pattern: &str, options: &SearchOptions) -> Result<Query, PatternError> {
        if pattern.is_empty() {
            return Err(PatternError {
                message: "Enter something to find.".to_string(),
            });
        }
        let body = if options.regex {
            pattern.to_string()
        } else {
            regex::escape(pattern)
        };
        let body = if options.whole_word {
            format!(r"\b(?:{body})\b")
        } else {
            body
        };
        let fast = regex::RegexBuilder::new(&body)
            .case_insensitive(options.ignore_case)
            .multi_line(true)
            .crlf(true)
            .build();
        let engine = match fast {
            Ok(fast) => Engine::Fast(fast),
            Err(first_error) => {
                let flags = if options.ignore_case {
                    "(?imR)"
                } else {
                    "(?mR)"
                };
                let backtracking =
                    fancy_regex::Regex::new(&format!("{flags}{body}")).map_err(|_| {
                        PatternError {
                            message: first_line(&first_error.to_string()),
                        }
                    })?;
                Engine::Backtracking(backtracking)
            }
        };
        let names = match &engine {
            Engine::Fast(r) => collect_names(r.capture_names()),
            Engine::Backtracking(r) => collect_names(r.capture_names()),
        };
        Ok(Query {
            engine,
            is_regex: options.regex,
            names,
        })
    }

    pub(crate) fn is_regex(&self) -> bool {
        self.is_regex
    }

    pub(crate) fn group_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    pub(crate) fn find_at(&self, text: &str, start: usize) -> Option<Range<usize>> {
        match &self.engine {
            Engine::Fast(r) => r.find_at(text, start).map(|m| m.range()),
            Engine::Backtracking(r) => r
                .find_from_pos(text, start)
                .ok()
                .flatten()
                .map(|m| m.range()),
        }
    }

    pub(crate) fn groups_at(&self, text: &str, start: usize) -> Groups {
        match &self.engine {
            Engine::Fast(r) => r
                .captures_at(text, start)
                .map(|c| c.iter().map(|m| m.map(|m| m.range())).collect())
                .unwrap_or_default(),
            Engine::Backtracking(r) => r
                .captures_from_pos(text, start)
                .ok()
                .flatten()
                .map(|c| c.iter().map(|m| m.map(|m| m.range())).collect())
                .unwrap_or_default(),
        }
    }
}

fn collect_names<'a>(names: impl Iterator<Item = Option<&'a str>>) -> HashMap<String, usize> {
    names
        .enumerate()
        .filter_map(|(index, name)| Some((name?.to_string(), index)))
        .collect()
}

fn first_line(message: &str) -> String {
    message
        .lines()
        .last()
        .unwrap_or("Invalid pattern")
        .trim()
        .to_string()
}
