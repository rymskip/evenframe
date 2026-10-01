//! The macroforge output the CLI writes per file, with a rule plugin that
//! gives every type a derive, the way a project that owns its derives runs it.

#[path = "support/binary.rs"]
mod binary;
#[path = "support/generated.rs"]
mod generated;
#[path = "support/plugins.rs"]
mod plugins;

use generated::generated;
use plugins::build_plugin;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn derive_plugin() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| build_plugin("macroforge_derive"))
}

/// The package the derive plugin's `Tracked` macro comes from.
const TRACKED_PACKAGE: &str = "@playground/macros";

/// The playground's config writing only macroforge, per file, with the derive
/// plugin.
fn macroforge_config(config: String) -> String {
    let (sections, _) = config
        .split_once("[typesync]")
        .expect("the playground config has a [typesync] section");
    let sections = sections.replacen(
        "[general]\n",
        &format!(
            "[general]\noutput_rule_plugins = {{ derive = {{ path = \"{}\" }} }}\n",
            derive_plugin().display()
        ),
        1,
    );
    format!(
        "{sections}[typesync]\noutput = {{ kind = \"macroforge\", dir = \"./src/bindings\", \
         mode = \"per_file\", macros = {{ Tracked = \"{TRACKED_PACKAGE}\" }} }}\n"
    )
}

fn configured_record_link(config: String) -> String {
    macroforge_config(config).replacen(
        "[typesync]",
        "[general.foreign_types.RecordLink]\nmacroforge = { type = \"RecordLink<{0}>\", \
         import = { from = \"./links.js\", name = \"RecordLink\" } }\n\n[typesync]",
        1,
    )
}

/// Each generated file's name and contents.
fn files(bindings: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = fs::read_dir(bindings)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// The one file that holds `declaration`.
fn file_declaring<'a>(files: &'a [(String, String)], declaration: &str) -> &'a (String, String) {
    let holders: Vec<&(String, String)> = files
        .iter()
        .filter(|(_, contents)| contents.contains(declaration))
        .collect();
    assert_eq!(
        holders.len(),
        1,
        "exactly one file should hold `{declaration}`, found {:?}",
        holders.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    holders[0]
}

#[test]
fn a_struct_variant_payload_is_a_named_type_in_its_enums_file() {
    let files =
        files(&generated("macroforge_default_record_link", macroforge_config).join("src/bindings"));
    let (enum_file, contents) = file_declaring(&files, "export type BookingKind =");
    let (payload_file, _) = file_declaring(&files, "export interface Package {");
    assert_eq!(
        payload_file, enum_file,
        "the payload lives with the enum that holds it"
    );

    let lines: Vec<&str> = contents.lines().collect();
    let payload = lines
        .iter()
        .position(|line| *line == "export interface Package {")
        .unwrap();
    assert!(
        lines[..payload]
            .iter()
            .rev()
            .take_while(|line| line.starts_with("/**"))
            .any(|line| line.contains("@derive(") && line.contains("Tracked")),
        "the payload carries the derive the rule plugin gives every type:\n{contents}"
    );
    assert!(
        contents.contains(&format!(
            "/** import macro {{Tracked}} from \"{TRACKED_PACKAGE}\"; */"
        )),
        "the file imports the derive from its configured package:\n{contents}"
    );
}

#[test]
fn without_a_record_link_entry_evenframe_declares_record_link() {
    let files =
        files(&generated("macroforge_default_record_link", macroforge_config).join("src/bindings"));
    let (link_file, link) = file_declaring(&files, "export type RecordLink<");
    assert!(
        link.contains("import type { RecordIdEncoded } from '../record-id.ts';")
            && link.contains("export type RecordLink<T> = RecordIdEncoded | T;"),
        "evenframe's RecordLink holds the playground's record id:\n{link}"
    );
    let (_, contents) = file_declaring(&files, "export type BookingKind =");
    let module = link_file.trim_end_matches(".ts");
    assert!(
        contents.contains(&format!("import type {{ RecordLink }} from './{module}';")),
        "the booking file imports evenframe's RecordLink from `{link_file}`:\n{contents}"
    );
}

#[test]
fn a_record_link_entry_points_imports_at_the_projects_record_link() {
    let files = files(
        &generated("macroforge_configured_record_link", configured_record_link)
            .join("src/bindings"),
    );
    assert!(
        files
            .iter()
            .all(|(_, contents)| !contents.contains("export type RecordLink")),
        "evenframe declares no RecordLink of its own when the project configures one"
    );
    let (_, contents) = file_declaring(&files, "export type BookingKind =");
    assert!(
        contents.contains("import type { RecordLink } from './links.js';"),
        "the booking file imports the configured RecordLink:\n{contents}"
    );
}
