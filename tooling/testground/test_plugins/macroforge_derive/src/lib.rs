//! Output-rule plugin that gives every type typesync writes a macroforge
//! derive, the way a project applies its derives across the board.

use evenframe_plugin::{OutputRulePluginOutput, define_output_rule_plugin};

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();
    if ctx.pipeline() != "Schemasync" {
        output
            .type_override
            .macroforge_derives
            .push("Tracked".to_string());
    }
    output
});
