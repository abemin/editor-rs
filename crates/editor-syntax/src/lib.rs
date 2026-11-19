//! Syntax coloring and block structure.

pub mod detect;
pub mod diff;
pub mod folding;
pub mod fortiweb_cli;
pub mod role;
pub mod treesitter;

pub use detect::{Language, detect};
pub use diff::highlight_diff_line;
pub use folding::{BlockIssue, CliStructure, FoldRange, cli_structure, matching_block_line};
pub use fortiweb_cli::{LineState, highlight_cli_line};
pub use role::{Role, Span};
pub use treesitter::{TreeHighlighter, input_edit};
