use crate::{
    mockmake::{Mockmaker, field_value::FieldValueGenerator},
    schemasync::table::TableConfig,
};
use convert_case::{Case, Casing};
use tracing::{debug, info};

impl Mockmaker {
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

            // Determine the record ID
            let record_id = if let Some(ids) = self.id_map.get(table_name) {
                if i < ids.len() {
                    ids[i].clone()
                } else {
                    format!("{}:{}", table_name.to_case(Case::Snake), i + 1)
                }
            } else {
                format!("{}:{}", table_name.to_case(Case::Snake), i + 1)
            };

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
                        .build()
                        .run();

                    // Check if this field needs null preservation
                    let needs_conditional =
                        super::needs_null_preservation(table_field, self.tables.get(table_name));

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
