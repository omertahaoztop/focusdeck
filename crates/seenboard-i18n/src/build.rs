use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use fluent_syntax::ast::Entry;
use fluent_syntax::parser;

pub fn generate(locales_dir: &str) {
    let dir =
        fs::canonicalize(locales_dir).unwrap_or_else(|e| panic!("locales dir {locales_dir}: {e}"));
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("locales dir {}: {e}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("locales dir entry: {e}"))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|ext| ext == "ftl"))
        .collect();
    files.sort();

    let mut sources = Vec::new();
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let tag = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| panic!("bad locale file name {}", path.display()));
        sources.push(format!(
            "({tag:?}, include_str!({:?}))",
            path.display().to_string()
        ));
    }

    let english = dir.join("en.ftl");
    let text =
        fs::read_to_string(&english).unwrap_or_else(|e| panic!("{}: {e}", english.display()));
    let resource = parser::parse(text.as_str())
        .unwrap_or_else(|(_, errors)| panic!("{}: {errors:?}", english.display()));

    let mut seen = BTreeSet::new();
    let mut consts = String::new();
    for entry in &resource.body {
        if let Entry::Message(message) = entry
            && seen.insert(message.id.name)
        {
            let name = message.id.name.replace('-', "_").to_uppercase();
            consts.push_str(&format!(
                "    pub const {name}: &str = {:?};\n",
                message.id.name
            ));
        }
    }

    let generated = format!(
        "#[allow(dead_code)]\npub const SOURCES: ::seenboard_i18n::Sources = &[{}];\n\n#[allow(dead_code)]\npub mod msg {{\n{consts}}}\n",
        sources.join(", ")
    );
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo for build scripts");
    let target = Path::new(&out_dir).join("seenboard_i18n.rs");
    fs::write(&target, generated).unwrap_or_else(|e| panic!("{}: {e}", target.display()));
}
