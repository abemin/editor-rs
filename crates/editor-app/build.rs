//! Generates the app's name and identifiers from `app.toml`.

use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).expect("app.toml");
    let table: toml::Table = text.parse().expect("valid app.toml");
    let get = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("app.toml needs {key}"))
            .to_string()
    };
    let code = format!(
        "pub const APP: AppConfig = AppConfig {{ display_name: {:?}, bundle_id: {:?}, version: {:?} }};\n",
        get("display_name"),
        get("bundle_id"),
        get("version")
    );
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("config.rs");
    std::fs::write(out, code).unwrap();
}
