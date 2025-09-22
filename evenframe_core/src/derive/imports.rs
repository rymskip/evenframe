use quote::quote;
use tracing::{debug, trace};

/// Generate imports for struct trait implementations
pub fn generate_struct_trait_imports() -> proc_macro2::TokenStream {
    trace!("Generating struct trait imports");
    quote! {
        use evenframe::{
            prelude::*,
            traits::EvenframePersistableStruct,
            types::{StructConfig, StructField, FieldType},
            validator::{StringValidator, Validator},
        };
    }
}

/// Generate imports for table configuration in persistable structs
pub fn generate_table_config_imports() -> proc_macro2::TokenStream {
    trace!("Generating table config imports");
    quote! {
        use evenframe::{
            prelude::*,
            config::EvenframeConfig,
            schemasync::{
                mockmake::MockGenerationConfig,
                compare::PreservationMode,
                TableConfig,
            },
        };
        use convert_case::{Case, Casing};
    }
}

/// Generate imports for parsing struct attributes
pub fn generate_struct_parsing_imports() -> proc_macro2::TokenStream {
    trace!("Generating struct parsing imports");
    quote! {
        use evenframe::{
            prelude::*,
            schemasync::{
                DefineConfig, Direction, EdgeConfig, PermissionsConfig,
            },
        };
    }
}

/// Generate imports for enum trait implementation (no longer needed - enums don't generate code)
pub fn generate_enum_trait_imports() -> proc_macro2::TokenStream {
    trace!("Generating enum trait imports (empty)");
    quote! {}
}

/// Generate imports needed for deserialization
pub fn generate_deserialize_imports() -> proc_macro2::TokenStream {
    trace!("Generating deserialize imports");
    quote! {

        use evenframe::{traits::EvenframeDeserialize, prelude::*};
    }
}

/// Generate registry imports for table registration
pub fn generate_registry_imports() -> proc_macro2::TokenStream {
    trace!("Generating registry imports");
    quote! {
        use evenframe::registry;
        use evenframe::prelude::linkme;
    }
}

/// Generate combined imports for struct implementations
pub fn generate_struct_imports() -> proc_macro2::TokenStream {
    debug!("Generating combined struct imports");
    let trait_imports = generate_struct_trait_imports();
    let table_imports = generate_table_config_imports();
    let parsing_imports = generate_struct_parsing_imports();
    let registry_imports = generate_registry_imports();

    debug!("Successfully generated combined struct imports");
    quote! {
        #trait_imports
        #table_imports
        #parsing_imports
        #registry_imports
    }
}

/// Generate all imports needed for enum implementations
pub fn generate_enum_imports() -> proc_macro2::TokenStream {
    debug!("Generating enum imports");
    generate_enum_trait_imports()
}
