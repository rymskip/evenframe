//! Type generation for build-time usage.

use crate::error::EvenframeError;
use crate::scan::{ScanConfig, build_all_configs, merge_tables_and_objects};
use crate::types::{AllConfigs, ForeignTypeRegistry};
use crate::typesync::checks::check_types;
use crate::typesync::config::TypesyncOutput;
use crate::typesync::output::{GeneratedFile, OutputTypes, render_outputs};
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
    config: ScanConfig,
}

impl TypeGenerator {
    /// Creates a new TypeGenerator with the given configuration.
    pub fn new(config: ScanConfig) -> Self {
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
        if outputs.is_empty() {
            return Err(EvenframeError::config(
                "No typesync output configured; specify `output` or `outputs` under [typesync], or set ScanConfig outputs",
            ));
        }
        info!("Starting type generation");
        let configs = build_all_configs(&self.config)?;
        let registry = ForeignTypeRegistry::from_config(&self.config.foreign_types);
        check_types(&configs, &registry)?;
        let AllConfigs {
            enums,
            tables,
            objects,
            newtypes,
        } = configs.for_typesync()?;
        let (tables_processed, structs_processed) = (tables.len(), objects.len());
        debug!(
            "Processing {} enums, {tables_processed} tables, {structs_processed} objects",
            enums.len()
        );
        let structs = merge_tables_and_objects(tables, objects);

        let types = OutputTypes::new(&structs, &enums, &newtypes, &registry)?;
        let targets: Vec<_> = outputs
            .iter()
            .map(|output| (output, output.resolve_dir(&self.config.scan_path)))
            .collect();
        let mut files = Vec::new();
        for output in render_outputs(&targets, None, &types)? {
            files.extend(output.write()?);
        }
        info!(
            "Generation complete. Generated {} files, {} changed",
            files.len(),
            files.iter().filter(|file| file.changed).count()
        );

        Ok(GenerationReport {
            files,
            enums_processed: enums.len(),
            structs_processed,
            tables_processed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TypeGenerator;
    use crate::scan::config::ScanConfig;

    #[test]
    fn generation_requires_an_explicit_output_before_scanning() {
        let failure = TypeGenerator::new(ScanConfig::default())
            .generate_all()
            .expect_err("an implicit generator must not run");
        assert!(
            failure
                .to_string()
                .contains("No typesync output configured")
        );
    }
}
