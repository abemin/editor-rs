//! What each piece of colored text means.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Plain,
    Keyword,
    Command,
    OperationalCommand,
    SectionName,
    EntityName,
    Property,
    On,
    Off,
    Action,
    SeverityLow,
    SeverityMedium,
    SeverityHigh,
    Number,
    Network,
    String,
    StringEscape,
    CertBlock,
    SecretTag,
    SecretValue,
    Comment,
    Framing,
    Type,
    Function,
    Variable,
    Operator,
    Tag,
    Attribute,
    DiffAdded,
    DiffRemoved,
    DiffHunk,
}

/// Colored text within one line, in byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub role: Role,
}
