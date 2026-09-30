//! Type generation for build-time usage.

use super::{BuildConfig, build_all_configs, filter_for_typesync, merge_tables_and_objects};
use crate::error::EvenframeError;
use crate::types::ForeignTypeRegistry;
use crate::typesync::checks::check_types;
use crate::typesync::config::TypesyncOutput;
use crate::typesync::output::{GeneratedFile, OutputTypes, render_output};
use tracing::{debug, info};

/// Report of the generation process.
#[derive(Debug, Clone)]
pub struct GenerationReport {
    /// List of files that were generated.
    pub files: Vec<GeneratedFile>,
    /// Number of enums processed.
    pub enums_processed: usize,
    /// Number of structs processed.
    pub structs_processed: usize,
    /// Number of tables processed.
    pub tables_processed: usize,
}

/// Generator for TypeScript types and schemas.
pub struct TypeGenerator {
    config: BuildConfig,
}

impl TypeGenerator {
    /// Creates a new TypeGenerator with the given configuration.
    pub fn new(config: BuildConfig) -> Self {
        Self { config }
    }

    /// Generates every configured output.
    pub fn generate_all(&self) -> Result<GenerationReport, EvenframeError> {
        self.generate(&self.config.outputs)
    }

    /// Generates one output, whether or not it is configured.
    pub fn generate_output(
        &self,
        output: &TypesyncOutput,
    ) -> Result<GenerationReport, EvenframeError> {
        self.generate(std::slice::from_ref(output))
    }

    fn generate(&self, outputs: &[TypesyncOutput]) -> Result<GenerationReport, EvenframeError> {
        info!("Starting type generation");
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let registry = ForeignTypeRegistry::from_config(&self.config.foreign_types);
        check_types(&enums, &tables, &objects, &registry)?;
        let (enums, tables, objects) = filter_for_typesync(enums, tables, objects);
        let structs = merge_tables_and_objects(&tables, &objects);
        debug!(
            "Processing {} enums, {} tables, {} objects",
            enums.len(),
            tables.len(),
            objects.len()
        );

        let types = OutputTypes {
            structs: &structs,
            enums: &enums,
            registry: &registry,
        };
        // Every output renders before any is written, so one that fails
        // leaves every output's files as they were.
        let rendered = outputs
            .iter()
            .map(|output| {
                render_output(
                    output,
                    &output.resolve_dir(&self.config.scan_path),
                    None,
                    &types,
                )
            })
            .collect::<Result<Vec<_>, EvenframeError>>()?;
        let mut files = Vec::new();
        for output in &rendered {
            files.extend(output.write()?);
        }
        info!("Generation complete. Generated {} files", files.len());

        Ok(GenerationReport {
            files,
            enums_processed: enums.len(),
            structs_processed: objects.len(),
            tables_processed: tables.len(),
        })
    }
}
