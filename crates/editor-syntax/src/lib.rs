//! Syntax coloring and block structure.

pub mod folding;
pub mod fortiweb_cli;
pub mod role;

pub use folding::{BlockIssue, CliStructure, FoldRange, cli_structure, matching_block_line};
pub use fortiweb_cli::{LineState, highlight_cli_line};
pub use role::{Role, Span};
