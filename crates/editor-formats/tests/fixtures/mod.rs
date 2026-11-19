//! Generates synthetic FortiWeb backups. Contains no real configuration data.

pub const SEP: &str = "-------FV000X-0.00-FW-build0000-000000-------2000-01-01 00:00:00-------0000000000000000AAAA---------";

pub struct FixtureSection {
    pub name: &'static str,
    pub kind: &'static str,
    pub encrypt: &'static str,
    pub compress: &'static str,
    pub payload: Vec<u8>,
}

pub fn text(name: &'static str, payload: &str) -> FixtureSection {
    FixtureSection {
        name,
        kind: "config",
        encrypt: "no",
        compress: "no",
        payload: payload.as_bytes().to_vec(),
    }
}

pub fn encrypted(payload: &[u8]) -> FixtureSection {
    FixtureSection {
        name: "/tmp/extend_tar_file",
        kind: "extend",
        encrypt: "yes",
        compress: "gzipped",
        payload: payload.to_vec(),
    }
}

pub fn header(sections: usize, extra: &str) -> String {
    format!(
        "[header]\nmagic=CC05E358\nversion=1.4\nimage_version=FV000X-0.00-FW-build0000-000000\nmodel=FV000X\ntype=full_config\nfile_number={sections}\nfile_split={SEP}\n{extra}[/header]\n"
    )
}

pub fn build_backup_with(extra: &str, sections: &[FixtureSection]) -> Vec<u8> {
    let mut out = header(sections.len(), extra).into_bytes();
    for s in sections {
        out.extend_from_slice(
            format!(
                "[file]\nname={}\ndomain=global\ntype={}\nencrypt={}\ncompress={}\n{SEP}\n",
                s.name, s.kind, s.encrypt, s.compress
            )
            .as_bytes(),
        );
        out.extend_from_slice(&s.payload);
        out.extend_from_slice(format!("{SEP}\n[/file]\n").as_bytes());
    }
    out
}

pub fn build_backup(sections: &[FixtureSection]) -> Vec<u8> {
    build_backup_with("", sections)
}
