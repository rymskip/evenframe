use quote::quote;

/// Generate imports for struct trait implementations
pub fn generate_struct_trait_imports() -> proc_macro2::TokenStream {
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
    quote! {
        use evenframe::{
            prelude::*,
            schemasync::{
                DefineConfig, Direction, EdgeConfig, PermissionsConfig,
            },
        };
    }
}

/// Generate imports needed for deserialization
pub fn generate_deserialize_imports() -> proc_macro2::TokenStream {
    quote! {
        use evenframe::{traits::EvenframeDeserialize, prelude::*};
    }
}

/// Generate registry imports for table registration
pub fn generate_registry_imports() -> proc_macro2::TokenStream {
    quote! {
        use evenframe::registry;
        use evenframe::prelude::linkme;
    }
}

/// Generate combined imports for struct implementations
pub fn generate_struct_imports() -> proc_macro2::TokenStream {
    let trait_imports = generate_struct_trait_imports();
    let table_imports = generate_table_config_imports();
    let parsing_imports = generate_struct_parsing_imports();
    let registry_imports = generate_registry_imports();

    quote! {
        #trait_imports
        #table_imports
        #parsing_imports
        #registry_imports
    }
}
