//! Serde types for WASM plugin communication.

use crate::error::EvenframeError;
use serde::{Deserialize, Serialize};

/// Input context sent to a field-level plugin.
#[derive(Debug, Serialize)]
pub struct PluginFieldInput {
    pub table_name: String,
    pub field_name: String,
    pub field_type: String,
    pub record_index: usize,
    pub total_records: usize,
    pub record_id: String,
}

/// Output from a field-level plugin.
#[derive(Debug, Deserialize)]
pub struct PluginFieldOutput {
    /// The generated SurrealQL-compatible value.
    pub value: Option<String>,
    /// Error message if generation failed.
    pub error: Option<String>,
    /// The plugin leaves the field to the default generator.
    #[serde(default)]
    pub declined: bool,
}

/// The error `evenframe_plugin` 0.5 declines a field with, in place of
/// `declined`.
const DECLINED_BEFORE_0_6: &str = "skip";

impl PluginFieldOutput {
    /// The value `plugin_name` gave, `None` when it declined the field, or
    /// an error when it failed or answered with nothing.
    pub fn into_value(self, plugin_name: &str) -> Result<Option<String>, EvenframeError> {
        if self.declined {
            return Ok(None);
        }
        match self.error.as_deref() {
            Some(DECLINED_BEFORE_0_6) => Ok(None),
            Some(error) => Err(EvenframeError::plugin(format!(
                "Plugin '{plugin_name}' error: {error}"
            ))),
            None => self.value.map(Some).ok_or_else(|| {
                EvenframeError::plugin(format!(
                    "Plugin '{plugin_name}' returned no value, error or decline"
                ))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PluginFieldOutput;

    fn answer(json: &str) -> Result<Option<String>, String> {
        serde_json::from_str::<PluginFieldOutput>(json)
            .expect("plugin output parses")
            .into_value("seed")
            .map_err(|error| error.to_string())
    }

    #[test]
    fn a_plugin_answers_with_a_value_a_decline_or_an_error() {
        assert_eq!(
            answer(r#"{ "value": "'ada'", "error": null }"#),
            Ok(Some("'ada'".to_string()))
        );
        assert_eq!(
            answer(r#"{ "value": null, "error": null, "declined": true }"#),
            Ok(None)
        );
        assert_eq!(answer(r#"{ "value": null, "error": "skip" }"#), Ok(None));
        let failed = answer(r#"{ "value": null, "error": "no seed for orders" }"#).unwrap_err();
        assert!(
            failed.contains("Plugin 'seed' error: no seed for orders"),
            "{failed}"
        );
        let empty = answer(r#"{ "value": null, "error": null }"#).unwrap_err();
        assert!(empty.contains("no value, error or decline"), "{empty}");
    }

    #[test]
    fn only_the_exact_skip_text_declines() {
        assert!(answer(r#"{ "value": null, "error": "skipped: no seed" }"#).is_err());
    }
}
