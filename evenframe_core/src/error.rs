use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvenframeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error in file {file}: {message}")]
    ParseError { file: PathBuf, message: String },

    #[error("Syn parse error: {0}")]
    SynParse(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Database error: {0}")]
    Database(Box<String>),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid path: {path}")]
    InvalidPath { path: PathBuf },

    #[error("Module not found: {module}")]
    ModuleNotFound { module: String },

    #[error("Type not found: {type_name}")]
    TypeNotFound { type_name: String },

    #[error("Field not found: {field} in type {type_name}")]
    FieldNotFound { field: String, type_name: String },

    #[error("Invalid field type: {message}")]
    InvalidFieldType { message: String },

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Schema sync error: {0}")]
    SchemaSync(String),

    #[error("Mock generation error: {0}")]
    MockGeneration(String),

    #[error("Permission error: {0}")]
    Permission(String),

    #[error("Workspace scan error: {0}")]
    WorkspaceScan(String),

    #[error("Maximum recursion depth ({depth}) reached at path: {path}")]
    MaxRecursionDepth { depth: usize, path: PathBuf },

    #[error("Invalid attribute: {0}")]
    InvalidAttribute(String),

    #[error("Duplicate definition: {0}")]
    DuplicateDefinition(String),

    #[error("Type conversion error: {0}")]
    TypeConversion(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid enum variant: {variant} for enum {enum_name}")]
    InvalidEnumVariant { variant: String, enum_name: String },

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Template error: {0}")]
    Template(String),

    #[error("Invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("Edge definition error: {0}")]
    EdgeDefinition(String),

    #[error("Table definition error: {0}")]
    TableDefinition(String),

    #[error(
        "Field definition error:\n{message}\nwork_stack: {work_stack}\nvalue_stack: {value_stack}\nitem: {item}\nvisited_types: {visited_types}\n"
    )]
    FieldDefinition {
        message: String,
        work_stack: String,
        value_stack: String,
        item: String,
        visited_types: String,
    },

    #[error("Access control error: {0}")]
    AccessControl(String),

    #[error("Query execution error: {0}")]
    QueryExecution(String),

    #[error("Invalid validator: {0}")]
    InvalidValidator(String),

    #[error("Type sync error: {0}")]
    TypeSync(String),

    #[error("Effect application error: {0}")]
    EffectApplication(String),

    #[error("Import error: {0}")]
    Import(String),

    #[error("Export error: {0}")]
    Export(String),

    #[error("Comparison error: {0}")]
    Comparison(String),

    #[error("Filter error: {0}")]
    Filter(String),

    #[error("Log error: {0}")]
    Log(String),

    #[error("Dependency resolution error: {0}")]
    DependencyResolution(String),

    #[error("Invalid configuration value: {key} = {value}")]
    InvalidConfigValue { key: String, value: String },

    #[error("Environment variable not set: {0}")]
    EnvVarNotSet(String),

    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Invalid regex pattern: {0}")]
    Regex(String),

    #[error("Timeout error: operation timed out after {seconds} seconds")]
    Timeout { seconds: u64 },

    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<syn::Error> for EvenframeError {
    fn from(err: syn::Error) -> Self {
        EvenframeError::SynParse(err.to_string())
    }
}

impl From<regex::Error> for EvenframeError {
    fn from(err: regex::Error) -> Self {
        EvenframeError::Regex(err.to_string())
    }
}

impl From<Box<dyn std::error::Error>> for EvenframeError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        EvenframeError::Unknown(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, EvenframeError>;

impl EvenframeError {
    pub fn parse_error(file: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        EvenframeError::ParseError {
            file: file.into(),
            message: message.into(),
        }
    }

    pub fn database(message: impl Into<String>) -> Self {
        EvenframeError::Database(Box::new(message.into()))
    }

    pub fn config(message: impl Into<String>) -> Self {
        EvenframeError::Config(message.into())
    }

    pub fn validation(message: impl Into<String>) -> Self {
        EvenframeError::Validation(message.into())
    }

    pub fn schema_sync(message: impl Into<String>) -> Self {
        EvenframeError::SchemaSync(message.into())
    }

    pub fn mock_generation(message: impl Into<String>) -> Self {
        EvenframeError::MockGeneration(message.into())
    }

    pub fn permission(message: impl Into<String>) -> Self {
        EvenframeError::Permission(message.into())
    }

    pub fn workspace_scan(message: impl Into<String>) -> Self {
        EvenframeError::WorkspaceScan(message.into())
    }

    pub fn invalid_attribute(message: impl Into<String>) -> Self {
        EvenframeError::InvalidAttribute(message.into())
    }

    pub fn duplicate_definition(message: impl Into<String>) -> Self {
        EvenframeError::DuplicateDefinition(message.into())
    }

    pub fn type_conversion(message: impl Into<String>) -> Self {
        EvenframeError::TypeConversion(message.into())
    }

    pub fn missing_field(field: impl Into<String>) -> Self {
        EvenframeError::MissingField(field.into())
    }

    pub fn circular_dependency(message: impl Into<String>) -> Self {
        EvenframeError::CircularDependency(message.into())
    }

    pub fn template(message: impl Into<String>) -> Self {
        EvenframeError::Template(message.into())
    }

    pub fn invalid_coordinate(message: impl Into<String>) -> Self {
        EvenframeError::InvalidCoordinate(message.into())
    }

    pub fn edge_definition(message: impl Into<String>) -> Self {
        EvenframeError::EdgeDefinition(message.into())
    }

    pub fn table_definition(message: impl Into<String>) -> Self {
        EvenframeError::TableDefinition(message.into())
    }

    pub fn access_control(message: impl Into<String>) -> Self {
        EvenframeError::AccessControl(message.into())
    }

    pub fn query_execution(message: impl Into<String>) -> Self {
        EvenframeError::QueryExecution(message.into())
    }

    pub fn invalid_validator(message: impl Into<String>) -> Self {
        EvenframeError::InvalidValidator(message.into())
    }

    pub fn type_sync(message: impl Into<String>) -> Self {
        EvenframeError::TypeSync(message.into())
    }

    pub fn effect_application(message: impl Into<String>) -> Self {
        EvenframeError::EffectApplication(message.into())
    }

    pub fn import(message: impl Into<String>) -> Self {
        EvenframeError::Import(message.into())
    }

    pub fn export(message: impl Into<String>) -> Self {
        EvenframeError::Export(message.into())
    }

    pub fn comparison(message: impl Into<String>) -> Self {
        EvenframeError::Comparison(message.into())
    }

    pub fn filter(message: impl Into<String>) -> Self {
        EvenframeError::Filter(message.into())
    }

    pub fn log(message: impl Into<String>) -> Self {
        EvenframeError::Log(message.into())
    }

    pub fn dependency_resolution(message: impl Into<String>) -> Self {
        EvenframeError::DependencyResolution(message.into())
    }

    pub fn serialization(message: impl Into<String>) -> Self {
        EvenframeError::Serialization(message.into())
    }

    pub fn deserialization(message: impl Into<String>) -> Self {
        EvenframeError::Deserialization(message.into())
    }

    pub fn network(message: impl Into<String>) -> Self {
        EvenframeError::Network(message.into())
    }

    pub fn unknown(message: impl Into<String>) -> Self {
        EvenframeError::Unknown(message.into())
    }
}
