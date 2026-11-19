//! Regex and literal search over document text.

mod engine;
mod query;
mod replace;

pub use engine::{Cancelled, SearchText, find_all, find_next};
pub use query::{PatternError, Query, SearchOptions};
pub use replace::{expand_replacement, replace_all};

#[cfg(test)]
mod tests;
