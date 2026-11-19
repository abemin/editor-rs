//! Choosing a language for a file.

use std::path::Path;

const CONTENT_LINES: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Plain,
    FortiWebCli,
    Json,
    Shell,
    Python,
    Html,
    Css,
    JavaScript,
    Php,
    Diff,
}

impl Language {
    pub const ALL: [Language; 10] = [
        Language::Plain,
        Language::FortiWebCli,
        Language::Json,
        Language::Shell,
        Language::Python,
        Language::Html,
        Language::Css,
        Language::JavaScript,
        Language::Php,
        Language::Diff,
    ];

    /// Name shown in the language menu and stored in the session.
    pub fn name(self) -> &'static str {
        match self {
            Language::Plain => "Plain Text",
            Language::FortiWebCli => "Fortinet CLI",
            Language::Json => "JSON",
            Language::Shell => "Shell Script",
            Language::Python => "Python",
            Language::Html => "HTML",
            Language::Css => "CSS",
            Language::JavaScript => "JavaScript",
            Language::Php => "PHP",
            Language::Diff => "Diff",
        }
    }

    pub fn from_name(name: &str) -> Option<Language> {
        if name == "FortiWeb CLI" {
            // The name used before other Fortinet products were supported.
            return Some(Language::FortiWebCli);
        }
        Language::ALL.into_iter().find(|l| l.name() == name)
    }
}

fn by_extension(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "json" => Language::Json,
        "sh" | "bash" | "zsh" => Language::Shell,
        "py" => Language::Python,
        "html" | "htm" => Language::Html,
        "css" => Language::Css,
        "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
        "php" => Language::Php,
        "diff" | "patch" => Language::Diff,
        _ => return None,
    })
}

fn by_shebang(first_line: &str) -> Option<Language> {
    let interpreter = first_line.strip_prefix("#!")?;
    let program = interpreter.split_whitespace().last()?.rsplit('/').next()?;
    Some(match program {
        p if p.starts_with("python") => Language::Python,
        "bash" | "sh" | "zsh" => Language::Shell,
        "node" => Language::JavaScript,
        p if p.starts_with("php") => Language::Php,
        _ => return None,
    })
}

fn looks_like_cli(text: &str) -> bool {
    text.lines().take(CONTENT_LINES).any(|line| {
        (line.starts_with("config ") && line.split_whitespace().count() >= 2)
            || line.starts_with("#config-version=")
            || line == "[header]"
    })
}

/// Picks a language from the file name, then from the first lines.
pub fn detect(path: Option<&Path>, first_lines: &str) -> Language {
    if let Some(language) = path.and_then(by_extension) {
        return language;
    }
    if let Some(language) = first_lines.lines().next().and_then(by_shebang) {
        return language;
    }
    if looks_like_cli(first_lines) {
        Language::FortiWebCli
    } else {
        Language::Plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detect_by_extension() {
        for (name, language) in [
            ("a.json", Language::Json),
            ("a.sh", Language::Shell),
            ("a.zsh", Language::Shell),
            ("a.py", Language::Python),
            ("a.html", Language::Html),
            ("a.htm", Language::Html),
            ("a.css", Language::Css),
            ("a.js", Language::JavaScript),
            ("a.mjs", Language::JavaScript),
            ("a.php", Language::Php),
            ("a.diff", Language::Diff),
            ("a.patch", Language::Diff),
            ("A.JSON", Language::Json),
        ] {
            assert_eq!(detect(Some(Path::new(name)), ""), language, "{name}");
        }
    }

    #[test]
    fn detect_shebang() {
        assert_eq!(detect(None, "#!/bin/bash\necho"), Language::Shell);
        assert_eq!(detect(None, "#!/usr/bin/env python3\n"), Language::Python);
        assert_eq!(
            detect(Some(Path::new("run")), "#!/usr/bin/env node\n"),
            Language::JavaScript
        );
    }

    #[test]
    fn detect_cli_by_content() {
        assert_eq!(
            detect(
                Some(Path::new("x.conf")),
                "config system global\n  set a 1\nend\n"
            ),
            Language::FortiWebCli
        );
        assert_eq!(
            detect(Some(Path::new("x.txt")), "#config-version=FV-1\n"),
            Language::FortiWebCli
        );
        assert_eq!(
            detect(None, "\n\nconfig waf signature\n"),
            Language::FortiWebCli
        );
        assert_eq!(
            detect(Some(Path::new("b.conf")), "[header]\nmagic=CC05E358\n"),
            Language::FortiWebCli
        );
    }

    #[test]
    fn unrelated_conf_is_plain() {
        assert_eq!(
            detect(Some(Path::new("x.conf")), "key=value\n"),
            Language::Plain
        );
        assert_eq!(detect(None, ""), Language::Plain);
    }

    #[test]
    fn cli_is_called_fortinet_and_old_sessions_still_restore() {
        assert_eq!(Language::FortiWebCli.name(), "Fortinet CLI");
        assert_eq!(
            Language::from_name("FortiWeb CLI"),
            Some(Language::FortiWebCli)
        );
    }

    #[test]
    fn detects_every_fortinet_product_header() {
        for (name, head) in [
            (
                "fortigate.conf",
                "#config-version=FGT60F-7.2.5-FW-build1517-230606:opmode=0:vdom=0\n",
            ),
            (
                "fortimail.cfg",
                "#config-version=FEVM-7.2-FW-build0399\nconfig system global\n",
            ),
            (
                "fortiadc.conf",
                "config system global\n  set hostname \"adc\"\nend\n",
            ),
            (
                "fortiweb.conf",
                "#config-version=FV-1KF-7.4.0-FW-build0123\n",
            ),
            ("backup", "[header]\nmagic=CC05E358\n"),
        ] {
            assert_eq!(
                detect(Some(Path::new(name)), head),
                Language::FortiWebCli,
                "{name}"
            );
        }
    }

    #[test]
    fn names_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_name(language.name()), Some(language));
        }
    }
}
