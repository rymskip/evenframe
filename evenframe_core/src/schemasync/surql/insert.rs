use crate::evenframe_log;
use crate::mockmake::Mockmaker;
use crate::mockmake::field_value::FieldValueGenerator;
use crate::schemasync::table::TableConfig;
use crate::types::FieldType;
use convert_case::{Case, Casing};
use tracing::{debug, info};

impl Mockmaker {
    pub fn generate_insert_statements(
        &self,
        table_name: &str,
        table_config: &TableConfig,
    ) -> String {
        info!(table_name = %table_name, "Generating insert statements for table");
        debug!("Table config: {:?}", table_config);
        let log_name = format!("insert_logs/{}.log", table_name);
        evenframe_log!("", log_name);

        let mut output = String::new();
        let config = self
            .tables
            .get(table_name)
            .expect("TableConfig was not found");

        evenframe_log!(
            format!(
                "Starting insert statement generation for table '{}'",
                table_name
            ),
            log_name,
            true
        );

        let n = config
            .mock_generation_config
            .as_ref()
            .map(|c| c.n)
            .unwrap_or(self.schemasync_config.mock_gen_config.default_record_count);

        evenframe_log!(
            format!("Will generate {} records for table {}", n, table_name),
            log_name,
            true
        );

        // Step 3: Generate individual INSERT statements for each record
        evenframe_log!(
            "Beginning individual INSERT statement generation",
            log_name,
            true
        );
        for i in 0..n {
            evenframe_log!(
                format!("Generating INSERT statement for record {}/{}", i + 1, n),
                log_name,
                true
            );
            let mut field_assignments = Vec::new();
            let mut update_assignments = Vec::new();

            // Determine the record ID
            let record_id = if let Some(ids) = self.id_map.get(table_name) {
                if i < ids.len() {
                    evenframe_log!(
                        format!("Using pre-generated ID from id_map: {}", ids[i]),
                        log_name,
                        true
                    );
                    ids[i].clone()
                } else {
                    let id = format!("{}:{}", table_name.to_case(Case::Snake), i + 1);
                    evenframe_log!(
                        format!("Generated fallback ID (index beyond id_map): {}", id),
                        log_name,
                        true
                    );
                    id
                }
            } else {
                let id = format!("{}:{}", table_name.to_case(Case::Snake), i + 1);
                evenframe_log!(
                    format!("Generated default ID (no id_map entry): {}", id),
                    log_name,
                    true
                );
                id
            };

            // Add the ID field
            field_assignments.push(format!("id: r'{}'", record_id));

            for table_field in &table_config.struct_config.fields {
                evenframe_log!(
                    format!("Processing field '{}'", table_field.field_name),
                    log_name,
                    true
                );
                // Skip explicit 'id' here because we already added it above
                if table_field.field_name == "id" {
                    evenframe_log!(
                        "Skipping field 'id' because it is set from record_id",
                        log_name,
                        true
                    );
                    continue;
                }
                if table_field.edge_config.is_none()
                    || (table_field.define_config.is_some()
                        && !table_field.define_config.as_ref().unwrap().should_skip)
                {
                    // Skip readonly fields
                    if let Some(ref define_config) = table_field.define_config
                        && let Some(true) = define_config.readonly
                    {
                        evenframe_log!(
                            format!("Skipping readonly field '{}'", table_field.field_name),
                            log_name,
                            true
                        );
                        continue;
                    }

                    let field_val = FieldValueGenerator::builder()
                        .field(table_field)
                        .id_index(&i)
                        .mockmaker(self)
                        .table_config(table_config)
                        .build()
                        .run();

                    // For relation tables, preserve existing 'in'/'out' endpoints if the edge already exists.
                    // Use a selective fetch from ONLY r'<id>' to keep prior endpoints; otherwise use the newly generated value.
                    let assigned_value = if table_config.relation.is_some()
                        && (table_field.field_name == "in" || table_field.field_name == "out")
                    {
                        format!(
                            "(IF (SELECT {fname} FROM ONLY r'{{rid}}') != NONE THEN (SELECT {fname} FROM ONLY r'{{rid}}').{fname} ELSE {gen} END)",
                            fname = table_field.field_name,
                            gen = field_val
                        )
                        .replace("{rid}", &record_id)
                    } else {
                        field_val
                    };

                    evenframe_log!(
                        format!(
                            "Generated value for field '{}': {}",
                            table_field.field_name, assigned_value
                        ),
                        log_name,
                        true
                    );
                    field_assignments.push(format!("{}: {assigned_value}", table_field.field_name));

                    // For relations, we don't update in/out fields
                    if !(table_config.relation.is_some()
                        && (table_field.field_name == "in" || table_field.field_name == "out"))
                    {
                        // Check if field is nullable
                        let is_nullable = matches!(&table_field.field_type, FieldType::Option(_));

                        if is_nullable {
                            // For nullable fields, preserve NULL values on update
                            update_assignments.push(format!(
                                "{} = (IF {} != NULL THEN $input.{} ELSE NULL END)",
                                table_field.field_name,
                                table_field.field_name,
                                table_field.field_name
                            ));
                        } else {
                            update_assignments.push(format!(
                                "{} = $input.{}",
                                table_field.field_name, table_field.field_name
                            ));
                        }
                    }
                }
            }

            let fields_str = field_assignments.join(", ");

            evenframe_log!(
                format!(
                    "Prepared {} field assignments and {} update assignments for record {}",
                    field_assignments.len(),
                    update_assignments.len(),
                    i
                ),
                log_name,
                true
            );

            // Generate the INSERT statement with ON DUPLICATE KEY UPDATE
            if table_config.relation.is_some() {
                evenframe_log!("Generating RELATION INSERT statement", log_name, true);
                // For relation tables, include ON DUPLICATE KEY UPDATE for non-in/out fields
                if !update_assignments.is_empty() {
                    let updates_str = update_assignments.join(", ");
                    evenframe_log!(
                        format!(
                            "Adding ON DUPLICATE KEY UPDATE clause with {} updates",
                            update_assignments.len()
                        ),
                        log_name,
                        true
                    );
                    output.push_str(&format!(
                        "INSERT RELATION INTO {} {{ {} }} ON DUPLICATE KEY UPDATE {};\n",
                        table_name, fields_str, updates_str
                    ));
                } else {
                    // No fields to update (only in/out or all readonly)
                    evenframe_log!(
                        "No updatable fields for relation, generating simple INSERT",
                        log_name,
                        true
                    );
                    output.push_str(&format!(
                        "INSERT RELATION INTO {} {{ {} }};\n",
                        table_name, fields_str
                    ));
                }
            } else if !update_assignments.is_empty() {
                evenframe_log!(
                    "Generating regular INSERT with ON DUPLICATE KEY UPDATE",
                    log_name,
                    true
                );
                let updates_str = update_assignments.join(", ");
                output.push_str(&format!(
                    "INSERT INTO {} {{ {} }} ON DUPLICATE KEY UPDATE {};\n",
                    table_name, fields_str, updates_str
                ));
            } else {
                // No fields to update (all readonly), just do a simple insert
                evenframe_log!(
                    "Generating simple INSERT (no updatable fields)",
                    log_name,
                    true
                );
                output.push_str(&format!(
                    "INSERT INTO {} {{ {} }};\n",
                    table_name, fields_str
                ));
            }
        }

        evenframe_log!(
            format!(
                "Successfully generated {} INSERT statements for table '{}' (total length: {} bytes)",
                n,
                table_name,
                output.len()
            ),
            log_name,
            true
        );

        output
    }
}
