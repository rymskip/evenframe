//! Type generation for build-time usage.

use super::{build_all_configs, merge_tables_and_objects, BuildConfig};
use crate::error::EvenframeError;
use crate::types::{StructConfig, TaggedUnion};
use crate::typesync::{
    arktype::generate_arktype_type_string, effect::generate_effect_schema_string,
    flatbuffers::generate_flatbuffers_schema_string,
    macroforge::generate_macroforge_type_string, protobuf::generate_protobuf_schema_string,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

/// The type of generator used to create a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorType {
    /// ArkType schema generator.
    ArkType,
    /// Effect-TS schema generator.
    Effect,
    /// Macroforge type generator.
    Macroforge,
    /// FlatBuffers schema generator.
    FlatBuffers,
    /// Protocol Buffers schema generator.
    Protobuf,
}

impl GeneratorType {
    /// Returns the default filename for this generator type.
    pub fn default_filename(&self) -> &'static str {
        match self {
            GeneratorType::ArkType => "arktype.ts",
            GeneratorType::Effect => "bindings.ts",
            GeneratorType::Macroforge => "macroforge.ts",
            GeneratorType::FlatBuffers => "schema.fbs",
            GeneratorType::Protobuf => "schema.proto",
        }
    }
}

/// Information about a generated file.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    /// The path where the file was written.
    pub path: PathBuf,
    /// The number of bytes written.
    pub bytes_written: usize,
    /// The type of generator that created this file.
    pub generator_type: GeneratorType,
}

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

impl GenerationReport {
    /// Creates a new empty report.
    fn new() -> Self {
        Self {
            files: Vec::new(),
            enums_processed: 0,
            structs_processed: 0,
            tables_processed: 0,
        }
    }

    /// Adds a generated file to the report.
    fn add_file(&mut self, file: GeneratedFile) {
        self.files.push(file);
    }
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

    /// Generates all enabled type outputs.
    pub fn generate_all(&self) -> Result<GenerationReport, EvenframeError> {
        info!("Starting type generation");
        let mut report = GenerationReport::new();

        // Build configs from the workspace
        let (enums, tables, objects) = build_all_configs(&self.config)?;

        report.enums_processed = enums.len();
        report.tables_processed = tables.len();
        report.structs_processed = objects.len();

        let structs = merge_tables_and_objects(&tables, &objects);

        debug!(
            "Processing {} enums, {} tables, {} objects",
            enums.len(),
            tables.len(),
            objects.len()
        );

        // Ensure output directory exists
        fs::create_dir_all(&self.config.output_path)?;

        // Generate each enabled type
        if self.config.arktype {
            let file = self.generate_arktype_internal(&structs, &enums)?;
            report.add_file(file);
        }

        if self.config.effect {
            let file = self.generate_effect_internal(&structs, &enums)?;
            report.add_file(file);
        }

        if self.config.macroforge {
            let file = self.generate_macroforge_internal(&structs, &enums)?;
            report.add_file(file);
        }

        if self.config.flatbuffers {
            let file = self.generate_flatbuffers_internal(&structs, &enums)?;
            report.add_file(file);
        }

        if self.config.protobuf {
            let file = self.generate_protobuf_internal(&structs, &enums)?;
            report.add_file(file);
        }

        info!(
            "Generation complete. Generated {} files",
            report.files.len()
        );

        Ok(report)
    }

    /// Generates only ArkType types.
    pub fn generate_arktype(&self) -> Result<GeneratedFile, EvenframeError> {
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let structs = merge_tables_and_objects(&tables, &objects);

        fs::create_dir_all(&self.config.output_path)?;

        self.generate_arktype_internal(&structs, &enums)
    }

    /// Generates only Effect-TS schemas.
    pub fn generate_effect(&self) -> Result<GeneratedFile, EvenframeError> {
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let structs = merge_tables_and_objects(&tables, &objects);

        fs::create_dir_all(&self.config.output_path)?;

        self.generate_effect_internal(&structs, &enums)
    }

    /// Generates only Macroforge types.
    pub fn generate_macroforge(&self) -> Result<GeneratedFile, EvenframeError> {
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let structs = merge_tables_and_objects(&tables, &objects);

        fs::create_dir_all(&self.config.output_path)?;

        self.generate_macroforge_internal(&structs, &enums)
    }

    /// Generates only FlatBuffers schema.
    pub fn generate_flatbuffers(&self) -> Result<GeneratedFile, EvenframeError> {
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let structs = merge_tables_and_objects(&tables, &objects);

        fs::create_dir_all(&self.config.output_path)?;

        self.generate_flatbuffers_internal(&structs, &enums)
    }

    /// Generates only Protocol Buffers schema.
    pub fn generate_protobuf(&self) -> Result<GeneratedFile, EvenframeError> {
        let (enums, tables, objects) = build_all_configs(&self.config)?;
        let structs = merge_tables_and_objects(&tables, &objects);

        fs::create_dir_all(&self.config.output_path)?;

        self.generate_protobuf_internal(&structs, &enums)
    }

    // Internal generation methods

    fn generate_arktype_internal(
        &self,
        structs: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<GeneratedFile, EvenframeError> {
        info!("Generating ArkType types");

        let content = generate_arktype_type_string(structs, enums, false);
        let full_content = format!(
            "import {{ scope }} from 'arktype';\n\n{}\n\n export const validator = scope({{\n  ...bindings.export(),\n}}).export();",
            content
        );

        let path = self
            .config
            .output_path
            .join(GeneratorType::ArkType.default_filename());

        let bytes_written = full_content.len();
        fs::write(&path, &full_content)?;

        info!("ArkType types written to {:?}", path);

        Ok(GeneratedFile {
            path,
            bytes_written,
            generator_type: GeneratorType::ArkType,
        })
    }

    fn generate_effect_internal(
        &self,
        structs: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<GeneratedFile, EvenframeError> {
        info!("Generating Effect schemas");

        let content = generate_effect_schema_string(structs, enums, false);
        let full_content = format!("import {{ Schema }} from \"effect\";\n\n{}", content);

        let path = self
            .config
            .output_path
            .join(GeneratorType::Effect.default_filename());

        let bytes_written = full_content.len();
        fs::write(&path, &full_content)?;

        info!("Effect schemas written to {:?}", path);

        Ok(GeneratedFile {
            path,
            bytes_written,
            generator_type: GeneratorType::Effect,
        })
    }

    fn generate_macroforge_internal(
        &self,
        structs: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<GeneratedFile, EvenframeError> {
        info!("Generating Macroforge types");

        let content = generate_macroforge_type_string(structs, enums, false);

        let path = self
            .config
            .output_path
            .join(GeneratorType::Macroforge.default_filename());

        let bytes_written = content.len();
        fs::write(&path, &content)?;

        info!("Macroforge types written to {:?}", path);

        Ok(GeneratedFile {
            path,
            bytes_written,
            generator_type: GeneratorType::Macroforge,
        })
    }

    fn generate_flatbuffers_internal(
        &self,
        structs: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<GeneratedFile, EvenframeError> {
        info!("Generating FlatBuffers schema");

        let content = generate_flatbuffers_schema_string(
            structs,
            enums,
            self.config.flatbuffers_namespace.as_deref(),
        );

        let path = self
            .config
            .output_path
            .join(GeneratorType::FlatBuffers.default_filename());

        let bytes_written = content.len();
        fs::write(&path, &content)?;

        info!("FlatBuffers schema written to {:?}", path);

        Ok(GeneratedFile {
            path,
            bytes_written,
            generator_type: GeneratorType::FlatBuffers,
        })
    }

    fn generate_protobuf_internal(
        &self,
        structs: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<GeneratedFile, EvenframeError> {
        info!("Generating Protocol Buffers schema");

        let content = generate_protobuf_schema_string(
            structs,
            enums,
            self.config.protobuf_package.as_deref(),
            self.config.protobuf_import_validate,
        );

        let path = self
            .config
            .output_path
            .join(GeneratorType::Protobuf.default_filename());

        let bytes_written = content.len();
        fs::write(&path, &content)?;

        info!("Protocol Buffers schema written to {:?}", path);

        Ok(GeneratedFile {
            path,
            bytes_written,
            generator_type: GeneratorType::Protobuf,
        })
    }
}
