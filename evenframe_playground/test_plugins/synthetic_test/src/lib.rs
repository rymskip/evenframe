//! Test synthetic-item WASM plugin used by the evenframe_playground e2e suite.
//!
//! Demonstrates the full-context plugin API: the plugin receives the
//! complete `structs`, `enums`, and `tables` maps as raw JSON, then
//! emits new items.

use evenframe_plugin::{
    SyntheticPluginOutput, define_synthetic_item_plugin, enum_item, option_of, string_type,
    struct_item, table_item,
};

define_synthetic_item_plugin!(|ctx: &SyntheticContext| {
    let mut output = SyntheticPluginOutput::default();

    // Pick the lexicographically smallest struct name for deterministic output.
    let Some(first_name) = ctx.structs.keys().min().cloned() else {
        return output;
    };

    // 1. Synthetic struct referencing the seed struct by name.
    let audit_body_field = format!("{}_audit_note", first_name);
    output.new_structs.push(struct_item(
        "SyntheticAudit",
        &[
            ("actor", string_type()),
            (audit_body_field.as_str(), string_type()),
            ("occurred_at", option_of(string_type())),
        ],
    ));

    // 2. Synthetic enum with three unit variants.
    output
        .new_enums
        .push(enum_item("SyntheticSeverity", &["Info", "Warning", "Critical"]));

    // 3. Synthetic persisted table.
    output.new_tables.push(table_item(
        "synthetic_ping",
        "SyntheticPing",
        &[
            ("id", string_type()),
            ("target", string_type()),
            ("latency_ms", option_of(string_type())),
        ],
    ));

    output
});
