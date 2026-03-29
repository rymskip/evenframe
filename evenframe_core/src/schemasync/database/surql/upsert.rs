use crate::{
    schemasync::mockmake::{Mockmaker, field_value::FieldValueGenerator},
    schemasync::table::TableConfig,
    types::{FieldType, StructField},
};
use convert_case::{Case, Casing};
use tracing::{debug, info};

/// Check if a field is nullable (wrapped in Option)
fn is_nullable_field(field: &StructField) -> bool {
    matches!(&field.field_type, FieldType::Option(_))
}

/// Check if a field is a nullable partial struct that needs conditional wrapping
fn is_nullable_partial_struct(field: &StructField, original_table: Option<&TableConfig>) -> bool {
    if let FieldType::Struct(_) = &field.field_type
        && let Some(table) = original_table
        && let Some(original_field) = table
            .struct_config
            .fields
            .iter()
            .find(|f| f.field_name == field.field_name)
    {
        return is_nullable_field(original_field);
    }
    false
}

/// Check if any field needs null-preserving conditional logic
fn needs_null_preservation(field: &StructField, original_table: Option<&TableConfig>) -> bool {
    if is_nullable_field(field) {
        return true;
    }
    if is_nullable_partial_struct(field, original_table) {
        return true;
    }
    false
}

impl Mockmaker<'_> {
    pub fn generate_upsert_statements(
        &self,
        table_name: &str,
        table_config: &TableConfig,
    ) -> String {
        info!(table_name = %table_name, "Generating upsert statements for table");
        debug!("Table config: {:?}", table_config);
        let mut output = String::new();
        let config = self
            .tables
            .get(table_name)
            .expect("TableConfig was not found");

        let n = config
            .mock_generation_config
            .as_ref()
            .map(|c| c.n)
            .unwrap_or(self.schemasync_config.mock_gen_config.default_record_count);

        // Step 3: Generate UPSERT statements for each record
        for i in 0..n {
            let mut field_assignments = Vec::new();

            // Determine the record ID (default from id_map)
            let default_record_id = if let Some(ids) = self.id_map.get(table_name) {
                if i < ids.len() {
                    ids[i].clone()
                } else {
                    format!("{}:{}", table_name.to_case(Case::Snake), i + 1)
                }
            } else {
                format!("{}:{}", table_name.to_case(Case::Snake), i + 1)
            };

            // Allow plugin to override the record ID
            #[cfg(feature = "wasm-plugins")]
            let record_id = {
                let plugin_name = table_config
                    .mock_generation_config
                    .as_ref()
                    .and_then(|c| c.plugin.as_ref());
                if let Some(plugin_name) = plugin_name {
                    if let Some(ref pm_cell) = self.plugin_manager {
                        let pm = &mut *pm_cell.borrow_mut();
                        let id_input =
                            crate::schemasync::mockmake::plugin_types::PluginFieldInput {
                                table_name: table_name.to_string(),
                                field_name: "id".to_string(),
                                field_type: "EvenframeRecordId".to_string(),
                                record_index: i,
                                total_records: n,
                                record_id: default_record_id.clone(),
                            };
                        match pm.generate_field_value(plugin_name, &id_input) {
                            Ok(val) => val,
                            Err(_) => default_record_id,
                        }
                    } else {
                        default_record_id
                    }
                } else {
                    default_record_id
                }
            };
            #[cfg(not(feature = "wasm-plugins"))]
            let record_id = default_record_id;

            // Then, process remaining fields that weren't coordinated
            for table_field in &table_config.struct_config.fields {
                if table_field.edge_config.is_none()
                    && (table_field.define_config.is_some()
                        && !table_field.define_config.as_ref().unwrap().should_skip
                        // Skip readonly fields
                        && table_field
                            .define_config
                            .as_ref()
                            .unwrap()
                            .readonly
                            .is_none())
                {
                    let field_val = FieldValueGenerator::builder()
                        .field(table_field)
                        .id_index(&i)
                        .mockmaker(self)
                        .table_config(table_config)
                        .registry(self.registry)
                        .build()
                        .run();

                    // Check if this field needs null preservation
                    let needs_conditional =
                        needs_null_preservation(table_field, self.tables.get(table_name));

                    if needs_conditional {
                        // Wrap in conditional to preserve NULL state
                        field_assignments.push(format!(
                            "{}: (IF {} != NULL THEN {} ELSE NULL END)",
                            table_field.field_name, table_field.field_name, field_val
                        ));
                    } else {
                        field_assignments.push(format!("{}: {field_val}", table_field.field_name));
                    }
                }
            }

            let fields_str = field_assignments.join(", ");

            // Generate UPSERT statement with CONTENT for each record
            if table_config.relation.is_some() {
                // For relation tables, we need special handling
                output.push_str(&format!("UPSERT {record_id} CONTENT {{ {fields_str} }};\n"));
            } else {
                // For regular tables, use UPSERT with CONTENT
                output.push_str(&format!("UPSERT {record_id} CONTENT {{ {fields_str} }};\n"));
            }
        }

        output
    }
}
