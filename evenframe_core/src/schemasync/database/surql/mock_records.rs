use crate::{
    error::Result,
    schemasync::mockmake::{Mockmaker, TableMocks, field_value::FieldValueGenerator},
    schemasync::table::TableConfig,
    types::{FieldType, StructField},
};
use tracing::{debug, info};

impl Mockmaker<'_> {
    /// The statements that write `mocks` to `table_name`: existing records
    /// get their rewritten fields replaced whole, and new records are created
    /// with every field.
    pub fn generate_mock_statements(&self, table_name: &str, mocks: &TableMocks) -> Result<String> {
        info!(table_name = %table_name, "Generating mock statements for table");
        debug!("Table mocks: {:?}", mocks);
        let table = self.table(table_name)?;
        let ids = self.id_map.get(table_name).map_or(&[][..], Vec::as_slice);
        let existing = ids.len().saturating_sub(mocks.new_records);
        let mut output = String::new();

        if !mocks.rewrite_fields.is_empty() {
            for (i, record_id) in ids.iter().enumerate().take(existing) {
                let assignments = self.rewrite_assignments(table, &mocks.rewrite_fields, i)?;
                if !assignments.is_empty() {
                    output.push_str(&format!("UPDATE {record_id} SET {assignments};\n"));
                }
            }
        }

        for (i, default_id) in ids.iter().enumerate().skip(existing) {
            #[cfg(feature = "wasm-plugins")]
            let record_id = self.plugin_record_id(table_name, table, i, default_id, ids.len());
            #[cfg(not(feature = "wasm-plugins"))]
            let record_id = default_id;
            let assignments = table
                .struct_config
                .fields
                .iter()
                .filter(|field| field.is_mock_written())
                .map(|field| {
                    Ok(format!(
                        "{}: {}",
                        field.field_name,
                        self.generate(table, field, i)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            if table.relation.is_some() {
                output.push_str(&format!(
                    "INSERT RELATION INTO {table_name} {{ id: {record_id}, {assignments} }};\n"
                ));
            } else {
                output.push_str(&format!(
                    "CREATE {record_id} CONTENT {{ {assignments} }};\n"
                ));
            }
        }

        Ok(output)
    }

    fn generate(&self, table: &TableConfig, field: &StructField, index: usize) -> Result<String> {
        FieldValueGenerator::builder()
            .field(field)
            .id_index(&index)
            .mockmaker(self)
            .table_config(table)
            .registry(self.registry)
            .build()
            .run()
    }

    /// `field = value` assignments rewriting the existing record at `index`
    /// in the id pool. SET replaces a value whole, where MERGE would keep an
    /// object's stale keys. An optional field that is NULL stays NULL, and
    /// a removed field (typed `Unit`) is set to NONE, which unsets it.
    fn rewrite_assignments(
        &self,
        table: &TableConfig,
        rewrite_fields: &[StructField],
        index: usize,
    ) -> Result<String> {
        Ok(rewrite_fields
            .iter()
            .filter(|field| field.is_mock_written())
            // A relation's endpoints are fixed once it exists.
            .filter(|field| {
                table.relation.is_none() || !matches!(field.field_name.as_str(), "in" | "out")
            })
            .map(|field| {
                let name = &field.field_name;
                match &field.field_type {
                    // A present value is rewritten with a present one,
                    // unless no present value can be generated.
                    FieldType::Option(inner) if !self.has_unfillable_link(inner) => {
                        let present = StructField {
                            field_type: inner.as_ref().clone(),
                            ..field.clone()
                        };
                        let value = self.generate(table, &present, index)?;
                        Ok(format!(
                            "{name} = (IF {name} != NULL THEN {value} ELSE NULL END)"
                        ))
                    }
                    _ => Ok(format!("{name} = {}", self.generate(table, field, index)?)),
                }
            })
            .collect::<Result<Vec<_>>>()?
            .join(", "))
    }

    /// The id of a new record: the pool's id, unless the table's mock plugin
    /// supplies one.
    #[cfg(feature = "wasm-plugins")]
    fn plugin_record_id(
        &self,
        table_name: &str,
        table: &TableConfig,
        index: usize,
        default_id: &str,
        total_records: usize,
    ) -> String {
        let (Some(plugin_name), Some(pm_cell)) = (
            table
                .mock_generation_config
                .as_ref()
                .and_then(|c| c.plugin.as_ref()),
            self.plugin_manager.as_ref(),
        ) else {
            return default_id.to_string();
        };
        let input = crate::schemasync::mockmake::plugin_types::PluginFieldInput {
            table_name: table_name.to_string(),
            field_name: "id".to_string(),
            field_type: "EvenframeRecordId".to_string(),
            record_index: index,
            total_records,
            record_id: default_id.to_string(),
        };
        match pm_cell
            .borrow_mut()
            .generate_field_value(plugin_name, &input)
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    "Plugin '{plugin_name}' gave no id for {default_id}, keeping it: {e}"
                );
                default_id.to_string()
            }
        }
    }
}
