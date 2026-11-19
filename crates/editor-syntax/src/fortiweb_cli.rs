//! Colors for Fortinet CLI configuration (FortiWeb, FortiGate, FortiMail,
//! FortiADC and other products that share the `config`/`edit` syntax).
//!
//! Words are colored by their position in a statement, so options that are
//! missing from the documentation still look right.

use crate::role::{Role, Span};

const ACTIONS: &[&str] = &[
    "alert",
    "deny",
    "alert_deny",
    "deny_no_log",
    "block-period",
    "block_period",
    "redirect",
    "send_403_forbidden",
    "send_HTTP_response",
    "client-id-block-period",
];
const SEVERITY_LOW: &[&str] = &[
    "Low",
    "low",
    "Info",
    "informational",
    "information",
    "notification",
    "debug",
];
const SEVERITY_MEDIUM: &[&str] = &["Medium", "medium", "moderate", "warning"];
const SEVERITY_HIGH: &[&str] = &[
    "High",
    "high",
    "critical",
    "severe",
    "substantial",
    "emergency",
    "error",
];
const FRAMING_KEYS: &[&str] = &[
    "magic=",
    "version=",
    "image_version=",
    "model=",
    "type=",
    "file_number=",
    "file_split=",
    "private_key=",
    "name=",
    "domain=",
    "encrypt=",
    "compress=",
];
const FRAMING_LINES: &[&str] = &["[header]", "[/header]", "[file]", "[/file]"];

/// Scanner state carried from one line to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineState(Inner);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Inner {
    #[default]
    Normal,
    InString {
        quote: u8,
        certificate: bool,
    },
    /// Base64 data between FortiMail's `#### FOLLOWING PART IS … ####` and
    /// `#### … END ####` markers.
    DataBlock,
}

fn starts_data_block(trimmed: &str) -> bool {
    trimmed.starts_with("#### FOLLOWING PART IS") && trimmed.ends_with("####")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Statement,
    Section,
    Entity,
    Option,
    Value,
    Secret,
    Plain,
}

struct Scanner<'a> {
    line: &'a str,
    bytes: &'a [u8],
    at: usize,
    spans: Vec<Span>,
}

impl Scanner<'_> {
    fn push(&mut self, start: usize, end: usize, role: Role) {
        if start < end && role != Role::Plain {
            self.spans.push(Span {
                range: start..end,
                role,
            });
        }
    }

    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            self.at += 1;
        }
    }

    fn word_end(&self) -> usize {
        let rest = &self.bytes[self.at..];
        self.at
            + rest
                .iter()
                .position(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                .unwrap_or(rest.len())
    }

    /// Scans string text up to the closing quote. Returns the state after this line.
    fn string_body(&mut self, start: usize, quote: u8, mut certificate: bool, base: Role) -> Inner {
        let mut run = start;
        let mut at = self.at;
        let end = self.line.trim_end_matches(['\r', '\n']).len();
        certificate |= self.line[at..end].contains("-----BEGIN");
        let role = if certificate { Role::CertBlock } else { base };
        while at < end {
            let b = self.bytes[at];
            if b == b'\\' && at + 1 < end {
                let next = at + 1 + self.line[at + 1..].chars().next().map_or(1, char::len_utf8);
                if role != Role::CertBlock && base == Role::String {
                    self.push(run, at, role);
                    self.push(at, next, Role::StringEscape);
                    run = next;
                }
                at = next;
            } else if b == quote {
                self.push(run, at + 1, role);
                self.at = at + 1;
                return Inner::Normal;
            } else {
                at += 1;
            }
        }
        self.push(run, end, role);
        self.at = end;
        Inner::InString { quote, certificate }
    }
}

fn is_ipv4(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            (1..=3).contains(&p.len())
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().is_ok_and(|n| n <= 255)
        })
}

fn is_network(word: &str) -> bool {
    let (address, prefix) = word.split_once('/').unwrap_or((word, "0"));
    if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if is_ipv4(address) {
        return true;
    }
    let colons = address.bytes().filter(|&b| b == b':').count();
    colons >= 2 && address.bytes().all(|b| b == b':' || b.is_ascii_hexdigit())
}

fn value_role(word: &str) -> Role {
    if word == "enable" {
        Role::On
    } else if word == "disable" {
        Role::Off
    } else if word == "ENC" {
        Role::SecretTag
    } else if ACTIONS.contains(&word) {
        Role::Action
    } else if SEVERITY_LOW.contains(&word) {
        Role::SeverityLow
    } else if SEVERITY_MEDIUM.contains(&word) {
        Role::SeverityMedium
    } else if SEVERITY_HIGH.contains(&word) {
        Role::SeverityHigh
    } else if word
        .strip_prefix('-')
        .unwrap_or(word)
        .bytes()
        .all(|b| b.is_ascii_digit())
        && word.bytes().any(|b| b.is_ascii_digit())
    {
        Role::Number
    } else if is_network(word) {
        Role::Network
    } else {
        Role::Plain
    }
}

fn is_framing(trimmed: &str) -> bool {
    FRAMING_LINES.contains(&trimmed)
        || trimmed.starts_with("-------")
        || FRAMING_KEYS.iter().any(|key| trimmed.starts_with(key))
}

/// Colors one line and returns the state for the next line.
pub fn highlight_cli_line(line: &str, state: &LineState) -> (Vec<Span>, LineState) {
    let mut s = Scanner {
        line,
        bytes: line.as_bytes(),
        at: 0,
        spans: Vec::new(),
    };
    if state.0 == Inner::DataBlock {
        let trimmed = line.trim();
        let start = line.len() - line.trim_start().len();
        let end = start + trimmed.len();
        if !trimmed.starts_with("####") {
            s.push(start, end, Role::CertBlock);
            return (s.spans, LineState(Inner::DataBlock));
        }
        s.push(start, end, Role::Comment);
        let next = if starts_data_block(trimmed) {
            Inner::DataBlock
        } else {
            Inner::Normal
        };
        return (s.spans, LineState(next));
    }
    if let Inner::InString { quote, certificate } = state.0 {
        let next = s.string_body(0, quote, certificate, Role::String);
        if next != Inner::Normal {
            return (s.spans, LineState(next));
        }
        return finish(s, Position::Value);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return (s.spans, LineState::default());
    }
    let start = line.len() - line.trim_start().len();
    let end = start + trimmed.len();
    if trimmed.starts_with('#') {
        s.push(start, end, Role::Comment);
        if starts_data_block(trimmed) {
            return (s.spans, LineState(Inner::DataBlock));
        }
        return (s.spans, LineState::default());
    }
    if is_framing(trimmed) {
        s.push(start, end, Role::Framing);
        return (s.spans, LineState::default());
    }
    finish(s, Position::Statement)
}

fn finish(mut s: Scanner<'_>, mut position: Position) -> (Vec<Span>, LineState) {
    loop {
        s.skip_space();
        let Some(&first) = s.bytes.get(s.at) else {
            break;
        };
        if matches!(first, b'\r' | b'\n') {
            break;
        }
        if matches!(first, b'"' | b'\'') && position != Position::Statement {
            let start = s.at;
            s.at += 1;
            let base = if position == Position::Entity {
                Role::EntityName
            } else {
                Role::String
            };
            let next = s.string_body(start, first, false, base);
            if next != Inner::Normal {
                return (s.spans, LineState(next));
            }
            position = advance(position);
            continue;
        }
        let end = s.word_end();
        let word = &s.line[s.at..end];
        let (role, next) = match position {
            Position::Statement => match word {
                "config" => (Role::Keyword, Position::Section),
                "edit" => (Role::Keyword, Position::Entity),
                "next" | "end" | "abort" => (Role::Keyword, Position::Plain),
                "set" | "unset" => (Role::Command, Position::Option),
                "delete" | "purge" => (Role::Command, Position::Entity),
                "get" | "show" | "execute" | "diagnose" => {
                    (Role::OperationalCommand, Position::Section)
                }
                _ => (Role::Plain, Position::Plain),
            },
            Position::Section => (Role::SectionName, Position::Section),
            Position::Entity => (Role::EntityName, Position::Entity),
            Position::Option => (Role::Property, Position::Value),
            Position::Secret => (Role::SecretValue, Position::Value),
            Position::Value => {
                let role = value_role(word);
                (
                    role,
                    if role == Role::SecretTag {
                        Position::Secret
                    } else {
                        Position::Value
                    },
                )
            }
            Position::Plain => (Role::Plain, Position::Plain),
        };
        s.push(s.at, end, role);
        s.at = end;
        position = next;
    }
    (s.spans, LineState::default())
}

fn advance(position: Position) -> Position {
    match position {
        Position::Option | Position::Secret => Position::Value,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_at(line: &str, word: &str) -> Role {
        let at = line.find(word).expect("word in line");
        let (spans, _) = highlight_cli_line(line, &LineState::default());
        spans
            .iter()
            .find(|s| s.range.start <= at && at < s.range.end)
            .map_or(Role::Plain, |s| s.role)
    }
    fn first_role(line: &str) -> Role {
        let word = line.split_whitespace().next().unwrap();
        role_at(line, word)
    }

    #[test]
    fn block_keywords() {
        for w in ["config system global", "edit 1", "next", "end", "  next"] {
            assert_eq!(first_role(w), Role::Keyword, "{w}");
        }
    }

    #[test]
    fn config_path_is_section_name() {
        assert_eq!(role_at("config system global", "system"), Role::SectionName);
        assert_eq!(role_at("config system global", "global"), Role::SectionName);
    }

    #[test]
    fn edit_name_is_entity() {
        assert_eq!(role_at("edit \"policy1\"", "\"policy1\""), Role::EntityName);
        assert_eq!(role_at("edit 3", "3"), Role::EntityName);
    }

    #[test]
    fn field_commands() {
        for w in ["set a 1", "unset a", "delete x", "purge"] {
            assert_eq!(first_role(w), Role::Command, "{w}");
        }
    }

    #[test]
    fn undocumented_option_is_still_a_property() {
        assert_eq!(
            role_at("  set made-up-option 1", "made-up-option"),
            Role::Property
        );
        assert_eq!(
            role_at("  unset made-up-option", "made-up-option"),
            Role::Property
        );
    }

    #[test]
    fn on_and_off() {
        assert_eq!(role_at("set status enable", "enable"), Role::On);
        assert_eq!(role_at("set status disable", "disable"), Role::Off);
    }

    #[test]
    fn action_and_severity_values() {
        assert_eq!(role_at("set action alert_deny", "alert_deny"), Role::Action);
        assert_eq!(
            role_at("set action block-period", "block-period"),
            Role::Action
        );
        assert_eq!(role_at("set severity Low", "Low"), Role::SeverityLow);
        assert_eq!(
            role_at("set severity Medium", "Medium"),
            Role::SeverityMedium
        );
        assert_eq!(role_at("set severity High", "High"), Role::SeverityHigh);
    }

    #[test]
    fn action_word_only_colors_in_value_position() {
        assert_eq!(role_at("set alert 1", "alert"), Role::Property);
    }

    #[test]
    fn numbers_and_network_values() {
        assert_eq!(role_at("set port 443", "443"), Role::Number);
        assert_eq!(role_at("set x -1", "-1"), Role::Number);
        for v in [
            "192.0.2.1",
            "192.0.2.0/24",
            "2001:db8::1",
            "::1",
            "00:11:22:33:44:55",
            "2001:db8::/32",
        ] {
            assert_eq!(role_at(&format!("set ip {v}"), v), Role::Network, "{v}");
        }
        assert_eq!(role_at("set v 999.1.1.1", "999.1.1.1"), Role::Plain);
        assert_eq!(role_at("set v a:b", "a:b"), Role::Plain);
    }

    #[test]
    fn operational_commands() {
        for w in [
            "execute reboot",
            "diagnose debug",
            "get system status",
            "show full-configuration",
        ] {
            assert_eq!(first_role(w), Role::OperationalCommand, "{w}");
        }
    }

    #[test]
    fn comment_line() {
        assert_eq!(first_role("#config-version=X"), Role::Comment);
        assert_eq!(role_at("  # note enable", "enable"), Role::Comment);
    }

    #[test]
    fn escaped_quote_does_not_close_string() {
        let line = r#"set a "x\"y" 5"#;
        assert_eq!(role_at(line, "x"), Role::String);
        assert_eq!(role_at(line, r#"\""#), Role::StringEscape);
        assert_eq!(role_at(line, "y"), Role::String);
        assert_eq!(role_at(line, "5"), Role::Number);
        assert_eq!(
            highlight_cli_line(line, &LineState::default()).1,
            LineState::default()
        );
    }

    #[test]
    fn apostrophe_inside_a_word_is_not_a_string() {
        let (_, state) = highlight_cli_line("set a don't", &LineState::default());
        assert_eq!(state, LineState::default());
    }

    #[test]
    fn string_spans_lines_with_state() {
        let (_, one) = highlight_cli_line("set msg \"first", &LineState::default());
        assert_ne!(one, LineState::default());
        let (spans, two) = highlight_cli_line("set enable inside", &one);
        assert_eq!(
            spans,
            vec![Span {
                range: 0..17,
                role: Role::String
            }]
        );
        assert_eq!(two, one);
        let (spans, three) = highlight_cli_line("last\" 7", &two);
        assert_eq!(three, LineState::default());
        assert_eq!(spans.last().unwrap().role, Role::Number);
    }

    #[test]
    fn certificate_inside_string_is_muted() {
        let (spans, one) = highlight_cli_line(
            "set certificate \"-----BEGIN CERTIFICATE-----",
            &LineState::default(),
        );
        assert_eq!(spans.last().unwrap().role, Role::CertBlock);
        let (spans, two) = highlight_cli_line("MIIBase64", &one);
        assert_eq!(spans[0].role, Role::CertBlock);
        let (spans, three) = highlight_cli_line("-----END CERTIFICATE-----\"", &two);
        assert_eq!(spans[0].role, Role::CertBlock);
        assert_eq!(three, LineState::default());
    }

    #[test]
    fn enc_secret_is_tagged_and_dimmed() {
        let line = "set password ENC abc123==";
        assert_eq!(role_at(line, "ENC"), Role::SecretTag);
        assert_eq!(role_at(line, "abc123=="), Role::SecretValue);
    }

    #[test]
    fn framing_lines_are_muted() {
        for w in [
            "[header]",
            "[/header]",
            "[file]",
            "[/file]",
            "-------FV000-------0---------",
            "file_split=abc",
            "encrypt=no",
        ] {
            assert_eq!(first_role(w), Role::Framing, "{w}");
        }
    }

    #[test]
    fn unknown_lines_stay_plain() {
        assert_eq!(first_role("hello world"), Role::Plain);
    }

    #[test]
    fn non_ascii_text_does_not_break_offsets() {
        let (spans, _) = highlight_cli_line("set é€ \"日本\" enable", &LineState::default());
        assert_eq!(spans.last().unwrap().role, Role::On);
    }

    #[test]
    fn twenty_thousand_char_line_highlights_under_5ms() {
        let line = format!("set msg \"{}\"", "x\\\" y ".repeat(3400));
        let start = std::time::Instant::now();
        highlight_cli_line(&line, &LineState::default());
        assert!(start.elapsed().as_millis() < 5, "{:?}", start.elapsed());
    }

    fn roles_of(text: &str) -> Vec<Vec<Role>> {
        let mut state = LineState::default();
        text.lines()
            .map(|line| {
                let (spans, next) = highlight_cli_line(line, &state);
                state = next;
                spans.iter().map(|s| s.role).collect()
            })
            .collect()
    }

    #[test]
    fn fortimail_data_blocks_are_colored_as_embedded_data() {
        let text = "end\n#### FOLLOWING PART IS USER UPLOAD LOGO ####\nQUJD+/9=\nend edit\n#### USER UPLOAD LOGO END ####\nconfig x\n";
        let roles = roles_of(text);
        assert_eq!(roles[1], vec![Role::Comment]);
        assert_eq!(roles[2], vec![Role::CertBlock]);
        assert_eq!(
            roles[3],
            vec![Role::CertBlock],
            "CLI words inside data stay data"
        );
        assert_eq!(roles[4], vec![Role::Comment]);
        assert_eq!(
            roles[5][0],
            Role::Keyword,
            "CLI resumes after the end marker"
        );
    }

    #[test]
    fn a_new_data_block_can_follow_an_end_marker_directly() {
        let text = "#### FOLLOWING PART IS A ####\nQQ==\n#### A END ####\n#### FOLLOWING PART IS DLP ####\nQg==\n#### DLP DATA END ####\n";
        let roles = roles_of(text);
        assert_eq!(roles[4], vec![Role::CertBlock]);
        assert_eq!(roles[5], vec![Role::Comment]);
    }

    #[test]
    fn ordinary_hash_comments_do_not_start_data() {
        let roles = roles_of("#### notes ####\nconfig x\n");
        assert_eq!(roles[1][0], Role::Keyword);
    }
}
