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

    #[error("Configuration file not found. Searched from {search_start:?} upward to root")]
    ConfigNotFound { search_start: PathBuf },

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

    pub fn config_error(message: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    // ==================== Display/Error Message Tests ====================

    #[test]
    fn test_io_error_display() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = EvenframeError::from(io_err);
        assert!(err.to_string().contains("IO error"));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn test_parse_error_display() {
        let err = EvenframeError::ParseError {
            file: PathBuf::from("/test/file.rs"),
            message: "unexpected token".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains("/test/file.rs"));
        assert!(display.contains("unexpected token"));
    }

    #[test]
    fn test_syn_parse_display() {
        let err = EvenframeError::SynParse("expected identifier".to_string());
        assert!(err.to_string().contains("expected identifier"));
    }

    #[test]
    fn test_database_error_display() {
        let err = EvenframeError::Database(Box::new("connection failed".to_string()));
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_config_error_display() {
        let err = EvenframeError::Config("invalid config value".to_string());
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("invalid config value"));
    }

    #[test]
    fn test_invalid_path_display() {
        let err = EvenframeError::InvalidPath {
            path: PathBuf::from("/invalid/path"),
        };
        assert!(err.to_string().contains("/invalid/path"));
    }

    #[test]
    fn test_module_not_found_display() {
        let err = EvenframeError::ModuleNotFound {
            module: "my_module".to_string(),
        };
        assert!(err.to_string().contains("my_module"));
    }

    #[test]
    fn test_type_not_found_display() {
        let err = EvenframeError::TypeNotFound {
            type_name: "MyStruct".to_string(),
        };
        assert!(err.to_string().contains("MyStruct"));
    }

    #[test]
    fn test_field_not_found_display() {
        let err = EvenframeError::FieldNotFound {
            field: "my_field".to_string(),
            type_name: "MyStruct".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains("my_field"));
        assert!(display.contains("MyStruct"));
    }

    #[test]
    fn test_invalid_field_type_display() {
        let err = EvenframeError::InvalidFieldType {
            message: "unsupported type".to_string(),
        };
        assert!(err.to_string().contains("unsupported type"));
    }

    #[test]
    fn test_validation_error_display() {
        let err = EvenframeError::Validation("value out of range".to_string());
        assert!(err.to_string().contains("Validation error"));
        assert!(err.to_string().contains("value out of range"));
    }

    #[test]
    fn test_schema_sync_error_display() {
        let err = EvenframeError::SchemaSync("sync failed".to_string());
        assert!(err.to_string().contains("Schema sync error"));
    }

    #[test]
    fn test_mock_generation_error_display() {
        let err = EvenframeError::MockGeneration("generation failed".to_string());
        assert!(err.to_string().contains("Mock generation error"));
    }

    #[test]
    fn test_max_recursion_depth_display() {
        let err = EvenframeError::MaxRecursionDepth {
            depth: 10,
            path: PathBuf::from("/deep/path"),
        };
        let display = err.to_string();
        assert!(display.contains("10"));
        assert!(display.contains("/deep/path"));
    }

    #[test]
    fn test_invalid_enum_variant_display() {
        let err = EvenframeError::InvalidEnumVariant {
            variant: "BadVariant".to_string(),
            enum_name: "MyEnum".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains("BadVariant"));
        assert!(display.contains("MyEnum"));
    }

    #[test]
    fn test_circular_dependency_display() {
        let err = EvenframeError::CircularDependency("A -> B -> A".to_string());
        assert!(err.to_string().contains("Circular dependency"));
        assert!(err.to_string().contains("A -> B -> A"));
    }

    #[test]
    fn test_field_definition_error_display() {
        let err = EvenframeError::FieldDefinition {
            message: "field error".to_string(),
            work_stack: "[A, B]".to_string(),
            value_stack: "[v1, v2]".to_string(),
            item: "item1".to_string(),
            visited_types: "[T1, T2]".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains("field error"));
        assert!(display.contains("[A, B]"));
        assert!(display.contains("[v1, v2]"));
        assert!(display.contains("item1"));
        assert!(display.contains("[T1, T2]"));
    }

    #[test]
    fn test_invalid_config_value_display() {
        let err = EvenframeError::InvalidConfigValue {
            key: "port".to_string(),
            value: "abc".to_string(),
        };
        let display = err.to_string();
        assert!(display.contains("port"));
        assert!(display.contains("abc"));
    }

    #[test]
    fn test_env_var_not_set_display() {
        let err = EvenframeError::EnvVarNotSet("MY_VAR".to_string());
        assert!(err.to_string().contains("MY_VAR"));
    }

    #[test]
    fn test_timeout_error_display() {
        let err = EvenframeError::Timeout { seconds: 30 };
        assert!(err.to_string().contains("30"));
        assert!(err.to_string().contains("seconds"));
    }

    #[test]
    fn test_unknown_error_display() {
        let err = EvenframeError::Unknown("something went wrong".to_string());
        assert!(err.to_string().contains("something went wrong"));
    }

    // ==================== From Conversion Tests ====================

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "permission denied");
        let err: EvenframeError = io_err.into();
        assert!(matches!(err, EvenframeError::Io(_)));
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_result: std::result::Result<String, _> = serde_json::from_str("{invalid}");
        let json_err = json_result.unwrap_err();
        let err: EvenframeError = json_err.into();
        assert!(matches!(err, EvenframeError::Json(_)));
    }

    #[test]
    fn test_from_toml_error() {
        let toml_result: std::result::Result<toml::Value, _> = toml::from_str("invalid = [");
        let toml_err = toml_result.unwrap_err();
        let err: EvenframeError = toml_err.into();
        assert!(matches!(err, EvenframeError::Toml(_)));
    }

    #[test]
    fn test_from_syn_error() {
        let syn_err = syn::Error::new(proc_macro2::Span::call_site(), "test error");
        let err: EvenframeError = syn_err.into();
        assert!(matches!(err, EvenframeError::SynParse(_)));
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_from_regex_error() {
        // Use a runtime string to prevent compile-time regex validation
        let invalid_pattern = String::from("[invalid");
        let regex_result = regex::Regex::new(&invalid_pattern);
        let regex_err = regex_result.unwrap_err();
        let err: EvenframeError = regex_err.into();
        assert!(matches!(err, EvenframeError::Regex(_)));
    }

    #[test]
    fn test_from_utf8_error() {
        let invalid_utf8 = vec![0xff, 0xfe];
        let utf8_result = String::from_utf8(invalid_utf8);
        let utf8_err = utf8_result.unwrap_err();
        let err: EvenframeError = utf8_err.into();
        assert!(matches!(err, EvenframeError::Utf8(_)));
    }

    #[test]
    fn test_from_boxed_dyn_error() {
        let boxed_err: Box<dyn std::error::Error> = Box::new(io::Error::other("boxed error"));
        let err: EvenframeError = boxed_err.into();
        assert!(matches!(err, EvenframeError::Unknown(_)));
        assert!(err.to_string().contains("boxed error"));
    }

    // ==================== Factory Method Tests ====================

    #[test]
    fn test_factory_parse_error() {
        let err = EvenframeError::parse_error("/path/to/file.rs", "syntax error");
        assert!(matches!(err, EvenframeError::ParseError { .. }));
        assert!(err.to_string().contains("/path/to/file.rs"));
        assert!(err.to_string().contains("syntax error"));
    }

    #[test]
    fn test_factory_parse_error_with_pathbuf() {
        let path = PathBuf::from("/another/path.rs");
        let err = EvenframeError::parse_error(path, "another error");
        assert!(err.to_string().contains("/another/path.rs"));
    }

    #[test]
    fn test_factory_database() {
        let err = EvenframeError::database("connection timeout");
        assert!(matches!(err, EvenframeError::Database(_)));
        assert!(err.to_string().contains("connection timeout"));
    }

    #[test]
    fn test_factory_config() {
        let err = EvenframeError::config("missing required field");
        assert!(matches!(err, EvenframeError::Config(_)));
        assert!(err.to_string().contains("missing required field"));
    }

    #[test]
    fn test_factory_validation() {
        let err = EvenframeError::validation("invalid email format");
        assert!(matches!(err, EvenframeError::Validation(_)));
        assert!(err.to_string().contains("invalid email format"));
    }

    #[test]
    fn test_factory_schema_sync() {
        let err = EvenframeError::schema_sync("table mismatch");
        assert!(matches!(err, EvenframeError::SchemaSync(_)));
        assert!(err.to_string().contains("table mismatch"));
    }

    #[test]
    fn test_factory_mock_generation() {
        let err = EvenframeError::mock_generation("failed to generate mocks");
        assert!(matches!(err, EvenframeError::MockGeneration(_)));
    }

    #[test]
    fn test_factory_permission() {
        let err = EvenframeError::permission("access denied");
        assert!(matches!(err, EvenframeError::Permission(_)));
    }

    #[test]
    fn test_factory_workspace_scan() {
        let err = EvenframeError::workspace_scan("no Cargo.toml found");
        assert!(matches!(err, EvenframeError::WorkspaceScan(_)));
    }

    #[test]
    fn test_factory_invalid_attribute() {
        let err = EvenframeError::invalid_attribute("unknown attribute");
        assert!(matches!(err, EvenframeError::InvalidAttribute(_)));
    }

    #[test]
    fn test_factory_duplicate_definition() {
        let err = EvenframeError::duplicate_definition("User already defined");
        assert!(matches!(err, EvenframeError::DuplicateDefinition(_)));
    }

    #[test]
    fn test_factory_type_conversion() {
        let err = EvenframeError::type_conversion("cannot convert to i32");
        assert!(matches!(err, EvenframeError::TypeConversion(_)));
    }

    #[test]
    fn test_factory_missing_field() {
        let err = EvenframeError::missing_field("name");
        assert!(matches!(err, EvenframeError::MissingField(_)));
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn test_factory_circular_dependency() {
        let err = EvenframeError::circular_dependency("A depends on B depends on A");
        assert!(matches!(err, EvenframeError::CircularDependency(_)));
    }

    #[test]
    fn test_factory_template() {
        let err = EvenframeError::template("template rendering failed");
        assert!(matches!(err, EvenframeError::Template(_)));
    }

    #[test]
    fn test_factory_invalid_coordinate() {
        let err = EvenframeError::invalid_coordinate("invalid x value");
        assert!(matches!(err, EvenframeError::InvalidCoordinate(_)));
    }

    #[test]
    fn test_factory_edge_definition() {
        let err = EvenframeError::edge_definition("missing from field");
        assert!(matches!(err, EvenframeError::EdgeDefinition(_)));
    }

    #[test]
    fn test_factory_table_definition() {
        let err = EvenframeError::table_definition("invalid table name");
        assert!(matches!(err, EvenframeError::TableDefinition(_)));
    }

    #[test]
    fn test_factory_access_control() {
        let err = EvenframeError::access_control("unauthorized");
        assert!(matches!(err, EvenframeError::AccessControl(_)));
    }

    #[test]
    fn test_factory_query_execution() {
        let err = EvenframeError::query_execution("query failed");
        assert!(matches!(err, EvenframeError::QueryExecution(_)));
    }

    #[test]
    fn test_factory_invalid_validator() {
        let err = EvenframeError::invalid_validator("unknown validator");
        assert!(matches!(err, EvenframeError::InvalidValidator(_)));
    }

    #[test]
    fn test_factory_type_sync() {
        let err = EvenframeError::type_sync("type mismatch");
        assert!(matches!(err, EvenframeError::TypeSync(_)));
    }

    #[test]
    fn test_factory_effect_application() {
        let err = EvenframeError::effect_application("effect failed");
        assert!(matches!(err, EvenframeError::EffectApplication(_)));
    }

    #[test]
    fn test_factory_import() {
        let err = EvenframeError::import("import failed");
        assert!(matches!(err, EvenframeError::Import(_)));
    }

    #[test]
    fn test_factory_export() {
        let err = EvenframeError::export("export failed");
        assert!(matches!(err, EvenframeError::Export(_)));
    }

    #[test]
    fn test_factory_comparison() {
        let err = EvenframeError::comparison("comparison failed");
        assert!(matches!(err, EvenframeError::Comparison(_)));
    }

    #[test]
    fn test_factory_filter() {
        let err = EvenframeError::filter("invalid filter");
        assert!(matches!(err, EvenframeError::Filter(_)));
    }

    #[test]
    fn test_factory_log() {
        let err = EvenframeError::log("log error");
        assert!(matches!(err, EvenframeError::Log(_)));
    }

    #[test]
    fn test_factory_dependency_resolution() {
        let err = EvenframeError::dependency_resolution("resolution failed");
        assert!(matches!(err, EvenframeError::DependencyResolution(_)));
    }

    #[test]
    fn test_factory_serialization() {
        let err = EvenframeError::serialization("serialization failed");
        assert!(matches!(err, EvenframeError::Serialization(_)));
    }

    #[test]
    fn test_factory_deserialization() {
        let err = EvenframeError::deserialization("deserialization failed");
        assert!(matches!(err, EvenframeError::Deserialization(_)));
    }

    #[test]
    fn test_factory_network() {
        let err = EvenframeError::network("network timeout");
        assert!(matches!(err, EvenframeError::Network(_)));
    }

    #[test]
    fn test_factory_unknown() {
        let err = EvenframeError::unknown("unknown error occurred");
        assert!(matches!(err, EvenframeError::Unknown(_)));
    }

    // ==================== Result Type Tests ====================

    #[test]
    fn test_result_type_ok() {
        let result: Result<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(42));
    }

    #[test]
    fn test_result_type_err() {
        let result: Result<i32> = Err(EvenframeError::unknown("test"));
        assert!(result.is_err());
    }

    // ==================== Debug Trait Tests ====================

    #[test]
    fn test_error_debug_impl() {
        let err = EvenframeError::Config("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("Config"));
    }

    // ==================== Error Trait Tests ====================

    #[test]
    fn test_error_trait_source_io() {
        use std::error::Error;
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let err = EvenframeError::from(io_err);
        // IO errors have a source
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_trait_source_simple() {
        use std::error::Error;
        let err = EvenframeError::Config("test".to_string());
        // Simple string errors don't have a source
        assert!(err.source().is_none());
    }
}
