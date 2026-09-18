use serde::{Deserialize, Serialize};

/// Whether to emit all types into a single file or split into per-type files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// All types in one file (default).
    #[default]
    Single,
    /// Each primary type gets its own file; exclusive dependents are co-located.
    PerFile,
}

/// TypeScript array syntax style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrayStyle {
    /// Shorthand syntax: `Type[]` — default
    #[default]
    Shorthand,
    /// Generic syntax: `Array<Type>`
    Generic,
}

/// Naming convention for generated per-file filenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileNamingConvention {
    /// PascalCase (e.g. `UserProfile.ts`)
    Pascal,
    /// kebab-case (e.g. `user-profile.ts`) — default
    #[default]
    Kebab,
    /// snake_case (e.g. `user_profile.ts`)
    Snake,
    /// camelCase (e.g. `userProfile.ts`)
    Camel,
}

/// Extension policy for the relative import specifiers in generated files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportExtensionStyle {
    /// Extensionless specifiers (`./user.svelte`) — resolvable only by
    /// lenient resolvers (TypeScript `bundler` mode, Deno sloppy-imports).
    /// Default, matching the historical output.
    #[default]
    Bare,
    /// Emit the extension the file has after transpilation
    /// (`./user.svelte.js`), so a packaged `dist/` resolves under strict
    /// node/Vite resolution with no post-processing. TypeScript maps the
    /// `.js` specifier back to the `.ts` source, so the same specifier
    /// works pre- and post-build.
    Js,
}

/// How to handle type name collisions across different source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionStrategy {
    /// Stop with a diagnostic error naming both files (default).
    #[default]
    Error,
    /// Automatically prefix the colliding type with its source filename in PascalCase.
    /// e.g. `PaymentMethod` in `invoice.rs` → `InvoicePaymentMethod`.
    AutoRename,
}

/// Per-file output configuration (used under `[typesync.output]`).
///
/// Missing keys, and a missing table, take the values of [`Default`], so the
/// serde and programmatic defaults cannot drift apart.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Single-file or per-file output mode.
    pub mode: OutputMode,
    /// Whether to generate a barrel `index.ts` that re-exports everything.
    pub barrel_file: bool,
    /// Naming convention for generated filenames.
    pub file_naming: FileNamingConvention,
    /// File extension for generated files (default: `.ts`).
    /// Use `.svelte.ts` for SvelteKit projects, etc.
    pub file_extension: String,
    /// TypeScript array syntax style (default: shorthand `Type[]`).
    /// Set to `generic` for `Array<Type>` syntax.
    pub array_style: ArrayStyle,
    /// Extension policy for relative import specifiers (default: `bare`).
    /// Set to `js` when the generated tree is consumed as a packaged
    /// `dist/` so its imports resolve without post-processing.
    pub import_extension: ImportExtensionStyle,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: OutputMode::default(),
            barrel_file: false,
            file_naming: FileNamingConvention::default(),
            file_extension: ".ts".to_string(),
            array_style: ArrayStyle::default(),
            import_extension: ImportExtensionStyle::default(),
        }
    }
}

/// A kind of generated output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    /// ArkType validator schemas.
    Arktype,
    /// Effect-TS schemas.
    Effect,
    /// Macroforge TypeScript interfaces.
    Macroforge,
    /// A FlatBuffers schema.
    Flatbuffers,
    /// A Protocol Buffers schema.
    Protobuf,
}

impl OutputKind {
    /// The name the config and the CLI use for this kind.
    pub fn name(self) -> &'static str {
        match self {
            OutputKind::Arktype => "arktype",
            OutputKind::Effect => "effect",
            OutputKind::Macroforge => "macroforge",
            OutputKind::Flatbuffers => "flatbuffers",
            OutputKind::Protobuf => "protobuf",
        }
    }

    /// The file a single-file output of this kind is written to.
    pub fn default_filename(self) -> &'static str {
        match self {
            OutputKind::Arktype => "arktype.ts",
            OutputKind::Effect => "bindings.ts",
            OutputKind::Macroforge => "macroforge.ts",
            OutputKind::Flatbuffers => "schema.fbs",
            OutputKind::Protobuf => "schema.proto",
        }
    }

    /// Whether this kind can write one file per type.
    pub fn supports_per_file(self) -> bool {
        matches!(self, OutputKind::Effect | OutputKind::Macroforge)
    }
}

impl std::fmt::Display for OutputKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// One generated output: which generator runs, where its files go and how
/// they are laid out.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypesyncOutput {
    pub kind: OutputKind,
    /// Directory the output is written to, relative to the project root
    /// unless absolute.
    pub dir: String,
    /// File name of a single-file output within `dir`, instead of the kind's
    /// standard name (such as `arktype.ts`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(flatten)]
    pub files: OutputConfig,
    /// FlatBuffers namespace (e.g. "com.example.app").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Protocol Buffers package (e.g. "com.example.app").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Whether to import validate.proto for validation rules (Protocol
    /// Buffers).
    #[serde(default)]
    pub import_validate: bool,
}

impl TypesyncOutput {
    /// An output of `kind` in `dir` with default settings.
    pub fn new(kind: OutputKind, dir: impl Into<String>) -> Self {
        Self {
            kind,
            dir: dir.into(),
            file: None,
            files: OutputConfig::default(),
            namespace: None,
            package: None,
            import_validate: false,
        }
    }

    /// The output directory, resolved against `project_root`.
    pub fn resolve_dir(&self, project_root: &std::path::Path) -> std::path::PathBuf {
        // Collecting the components drops the `./` that a configured path
        // such as `./src/generated/` leaves inside the joined path.
        project_root.join(&self.dir).components().collect()
    }

    /// Rejects settings the generator for this kind cannot honor.
    pub fn validate(&self) -> Result<(), String> {
        let kind = self.kind;
        if self.file.is_some() && self.files.mode == OutputMode::PerFile {
            return Err(format!(
                "`file` names a single output file, but the `{kind}` output is per-file"
            ));
        }
        if self.files.mode == OutputMode::PerFile && !kind.supports_per_file() {
            return Err(format!(
                "the `{kind}` output cannot use mode = \"per_file\"; only effect and macroforge can"
            ));
        }
        if self.namespace.is_some() && kind != OutputKind::Flatbuffers {
            return Err(format!(
                "`namespace` only applies to flatbuffers outputs, not `{kind}`"
            ));
        }
        if (self.package.is_some() || self.import_validate) && kind != OutputKind::Protobuf {
            return Err(format!(
                "`package` and `import_validate` only apply to protobuf outputs, not `{kind}`"
            ));
        }
        Ok(())
    }
}

/// Configuration for Typesync operations (TypeScript/Effect type generation)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "TypesyncToml")]
pub struct TypesyncConfig {
    /// Every configured output, from `output` (one) or `outputs` (several).
    pub outputs: Vec<TypesyncOutput>,
    /// How to handle type name collisions across different source files.
    pub collision_strategy: CollisionStrategy,
}

/// `[typesync]` as written: a single `output` table or an `outputs` array.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypesyncToml {
    output: Option<TypesyncOutput>,
    #[serde(default)]
    outputs: Vec<TypesyncOutput>,
    #[serde(default)]
    collision_strategy: CollisionStrategy,
}

impl TryFrom<TypesyncToml> for TypesyncConfig {
    type Error = String;

    fn try_from(toml: TypesyncToml) -> Result<Self, Self::Error> {
        let outputs = match toml.output {
            Some(_) if !toml.outputs.is_empty() => {
                return Err("`output` and `outputs` cannot both be set".to_string());
            }
            Some(output) => vec![output],
            None => toml.outputs,
        };
        for output in &outputs {
            output.validate()?;
        }
        Ok(Self {
            outputs,
            collision_strategy: toml.collision_strategy,
        })
    }
}
