//! `config`/`end` and `edit`/`next` blocks.

use crate::fortiweb_cli::{LineState, highlight_cli_line};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}

/// A block that does not close properly. Shown subtly, never treated as an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockIssue {
    pub line: usize,
    pub message: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliStructure {
    pub folds: Vec<FoldRange>,
    pub issues: Vec<BlockIssue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Config,
    Edit,
}

const MISSING_END: &str = "This config block has no matching end.";
const MISSING_NEXT: &str = "This edit block has no matching next.";
const STRAY_NEXT: &str = "This next has no matching edit.";
const STRAY_END: &str = "This end has no matching config.";

/// Finds the block structure of a CLI file.
pub fn cli_structure<'a>(lines: impl Iterator<Item = &'a str>) -> CliStructure {
    let mut result = CliStructure::default();
    let mut stack: Vec<(Block, usize)> = Vec::new();
    let mut state = LineState::default();
    for (number, line) in lines.enumerate() {
        let at_statement = state == LineState::default();
        state = highlight_cli_line(line, &state).1;
        if !at_statement {
            continue;
        }
        match line.split_whitespace().next() {
            Some("config") => stack.push((Block::Config, number)),
            Some("edit") => stack.push((Block::Edit, number)),
            Some("next") => match stack.last() {
                Some(&(Block::Edit, start)) => {
                    stack.pop();
                    result.folds.push(FoldRange {
                        start_line: start,
                        end_line: number,
                    });
                }
                _ => result.issues.push(BlockIssue {
                    line: number,
                    message: STRAY_NEXT,
                }),
            },
            Some("end") => {
                while let Some(&(Block::Edit, start)) = stack.last() {
                    stack.pop();
                    result.issues.push(BlockIssue {
                        line: start,
                        message: MISSING_NEXT,
                    });
                }
                match stack.pop() {
                    Some((_, start)) => result.folds.push(FoldRange {
                        start_line: start,
                        end_line: number,
                    }),
                    None => result.issues.push(BlockIssue {
                        line: number,
                        message: STRAY_END,
                    }),
                }
            }
            _ => {}
        }
    }
    for (block, line) in stack {
        let message = if block == Block::Config {
            MISSING_END
        } else {
            MISSING_NEXT
        };
        result.issues.push(BlockIssue { line, message });
    }
    result.issues.sort_by_key(|issue| issue.line);
    result
}

/// The line that opens or closes the block at `line`.
pub fn matching_block_line(structure: &CliStructure, line: usize) -> Option<usize> {
    structure.folds.iter().find_map(|fold| {
        if fold.start_line == line {
            Some(fold.end_line)
        } else if fold.end_line == line {
            Some(fold.start_line)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(text: &str) -> CliStructure {
        cli_structure(text.lines())
    }

    #[test]
    fn folds_nest() {
        let s = structure("config a\n  edit 1\n    config b\n    end\n  next\nend\n");
        let mut folds: Vec<_> = s.folds.iter().map(|f| (f.start_line, f.end_line)).collect();
        folds.sort();
        assert_eq!(folds, vec![(0, 5), (1, 4), (2, 3)]);
        assert!(s.issues.is_empty());
    }

    #[test]
    fn block_issues_are_reported_never_fatal() {
        let s = structure("config a\n  edit 1\nend\nnext\nend\n");
        let lines: Vec<_> = s.issues.iter().map(|i| i.line).collect();
        assert_eq!(lines, vec![1, 3, 4]);
        assert_eq!(structure("config a\n").issues[0].line, 0);
    }

    #[test]
    fn block_words_inside_strings_are_ignored() {
        let s = structure("config a\n  set msg \"first\nend\nnext\nlast\"\nend\n");
        assert!(s.issues.is_empty());
        assert_eq!(s.folds.len(), 1);
    }

    #[test]
    fn matching_line_works_both_ways() {
        let s = structure("config a\n  edit 1\n  next\nend\n");
        assert_eq!(matching_block_line(&s, 0), Some(3));
        assert_eq!(matching_block_line(&s, 3), Some(0));
        assert_eq!(matching_block_line(&s, 2), Some(1));
        assert_eq!(matching_block_line(&s, 9), None);
    }
}
