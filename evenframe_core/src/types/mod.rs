mod all_configs;
mod field_type;
pub mod foreign_type_registry;
mod newtype;
#[cfg(feature = "surrealdb-types")]
mod record_id;
#[cfg(feature = "surrealdb-types")]
mod record_link;
mod text_form;

pub use crate::types::field_type::{FieldType, PathNames, STD_DURATION_PATHS, TextFormKind};
#[cfg(feature = "schemadump")]
use crate::{
    Result,
    schemasync::{TableConfig, table::surql_ident},
};
use crate::{
    schemasync::mockmake::mock_format::MockFormat,
    schemasync::{DefineConfig, EdgeConfig},
    validator::{Validator, ValidatorOverrides, morph::Morph},
};
pub use all_configs::{AllConfigs, SchemasyncTypes};
use convert_case::{Case, Casing};
pub use foreign_type_registry::ForeignTypeRegistry;
pub use newtype::{DeclaredTypes, FieldOwner, NewtypeConfig, NewtypeKind, desugar_newtypes};
#[cfg(feature = "surrealdb-types")]
pub use record_link::RecordLink;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(feature = "schemadump")]
use std::collections::HashSet;
pub use text_form::{EpochMillis, FromText, IsoDate, JsonText, TextForm};

/// Which pipeline(s) a type participates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Pipeline {
    #[default]
    Both,
    Typesync,
    Schemasync,
}

impl Pipeline {
    pub fn includes_typesync(&self) -> bool {
        matches!(self, Pipeline::Both | Pipeline::Typesync)
    }

    pub fn includes_schemasync(&self) -> bool {
        matches!(self, Pipeline::Both | Pipeline::Schemasync)
    }

    /// The part of the pipeline that is typesync, if any.
    pub fn typesync_part(self) -> Option<Pipeline> {
        self.includes_typesync().then_some(Pipeline::Typesync)
    }

    /// The part of the pipeline that is schemasync, if any.
    pub fn schemasync_part(self) -> Option<Pipeline> {
        self.includes_schemasync().then_some(Pipeline::Schemasync)
    }
}

impl quote::ToTokens for Pipeline {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            Pipeline::Both => quote::quote! { ::evenframe::types::Pipeline::Both },
            Pipeline::Typesync => quote::quote! { ::evenframe::types::Pipeline::Typesync },
            Pipeline::Schemasync => quote::quote! { ::evenframe::types::Pipeline::Schemasync },
        };
        tokens.extend(variant_tokens);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum EnumRepresentation {
    #[default]
    ExternallyTagged,
    InternallyTagged {
        tag: String,
    },
    AdjacentlyTagged {
        tag: String,
        content: String,
    },
    Untagged,
}

impl quote::ToTokens for EnumRepresentation {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let path = quote::quote! { ::evenframe::types::EnumRepresentation };
        tokens.extend(match self {
            EnumRepresentation::ExternallyTagged => quote::quote! { #path::ExternallyTagged },
            EnumRepresentation::InternallyTagged { tag } => {
                quote::quote! { #path::InternallyTagged { tag: #tag.to_owned() } }
            }
            EnumRepresentation::AdjacentlyTagged { tag, content } => quote::quote! {
                #path::AdjacentlyTagged { tag: #tag.to_owned(), content: #content.to_owned() }
            },
            EnumRepresentation::Untagged => quote::quote! { #path::Untagged },
        });
    }
}

/// Names outside Rust: serde in JSON, TypeScript in generated outputs,
/// and SurrealValue in the database.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Wire {
    /// An explicit TypeScript name, or the configured serde fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typescript: Option<String>,
    #[serde(default)]
    pub serde: Option<String>,
    #[serde(default)]
    pub surreal: Option<String>,
    /// Serde neither writes nor reads it, so JSON never carries it.
    #[serde(default)]
    pub serde_skipped: bool,
    /// The JSON may lack the key: serde leaves it out under
    /// `skip_serializing_if`, or skips it in one direction only.
    #[serde(default)]
    pub serde_optional: bool,
    /// A variant's `#[serde(untagged)]`: serde writes its payload bare,
    /// whatever its enum's representation, and reads it after every tagged
    /// variant.
    #[serde(default)]
    pub serde_untagged: bool,
    /// A field's `#[serde(flatten)]`: serde writes what it holds beside its
    /// siblings rather than under its own key.
    #[serde(default)]
    pub serde_flatten: bool,
    #[serde(default)]
    pub storage: Storage,
}

/// How the database stores an item, where that differs from serde's JSON:
/// serde's shape, overridden by the item's `#[surreal(...)]` keys.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Storage {
    /// A field never written to the database, so the schema defines nothing
    /// for it; a variant stored as NONE.
    #[serde(default)]
    pub skipped: bool,
    /// A field stored beside its siblings, by serde's or `#[surreal]`'s
    /// flatten.
    #[serde(default)]
    pub flatten: bool,
    /// A field stored through serde, in a shape the schema does not describe:
    /// `#[surreal(wrap)]`, or serde's `with`.
    #[serde(default)]
    pub opaque: bool,
    /// Each element of a tuple variant stored through serde.
    #[serde(default)]
    pub opaque_elements: Vec<bool>,
    /// A variant stored as its enum's `#[surreal]` representation says, in
    /// place of serde's.
    #[serde(default)]
    pub representation: Option<EnumRepresentation>,
    /// A unit variant stored as this SurrealQL literal, by `#[surreal(value)]`
    /// or, for an untagged one, serde's null.
    #[serde(default)]
    pub value: Option<String>,
    /// A unit variant read for any value no other variant reads.
    #[serde(default)]
    pub other: bool,
    /// A tuple variant of one element stored as an array.
    #[serde(default)]
    pub tuple: bool,
    /// When an adjacently tagged variant's content key is stored.
    #[serde(default)]
    pub content: ContentStorage,
}

/// When an adjacently tagged variant's content key is stored.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentStorage {
    #[default]
    Always,
    /// Left out when `#[surreal(skip_content_if)]` holds for the content.
    Sometimes,
    /// Never, by `#[surreal(skip_content)]`.
    Never,
}

impl quote::ToTokens for Storage {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let Storage {
            skipped,
            flatten,
            opaque,
            opaque_elements,
            representation,
            value,
            other,
            tuple,
            content,
        } = self;
        let representation = match representation {
            Some(representation) => quote::quote! { ::std::option::Option::Some(#representation) },
            None => quote::quote! { ::std::option::Option::None },
        };
        let value = match value {
            Some(value) => quote::quote! { ::std::option::Option::Some(#value.to_owned()) },
            None => quote::quote! { ::std::option::Option::None },
        };
        let content = match content {
            ContentStorage::Always => quote::quote! { ::evenframe::types::ContentStorage::Always },
            ContentStorage::Sometimes => {
                quote::quote! { ::evenframe::types::ContentStorage::Sometimes }
            }
            ContentStorage::Never => quote::quote! { ::evenframe::types::ContentStorage::Never },
        };
        tokens.extend(quote::quote! {
            ::evenframe::types::Storage {
                skipped: #skipped,
                flatten: #flatten,
                opaque: #opaque,
                opaque_elements: vec![#(#opaque_elements),*],
                representation: #representation,
                value: #value,
                other: #other,
                tuple: #tuple,
                content: #content,
            }
        });
    }
}

impl quote::ToTokens for Wire {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let optional = |name: &Option<String>| match name {
            Some(name) => quote::quote! { Some(#name.to_string()) },
            None => quote::quote! { None },
        };
        let serde = optional(&self.serde);
        let typescript = optional(&self.typescript);
        let surreal = optional(&self.surreal);
        let serde_skipped = self.serde_skipped;
        let serde_optional = self.serde_optional;
        let serde_untagged = self.serde_untagged;
        let serde_flatten = self.serde_flatten;
        let storage = &self.storage;
        tokens.extend(quote::quote! {
            ::evenframe::types::Wire {
                typescript: #typescript,
                serde: #serde,
                surreal: #surreal,
                serde_skipped: #serde_skipped,
                serde_optional: #serde_optional,
                serde_untagged: #serde_untagged,
                serde_flatten: #serde_flatten,
                storage: #storage,
            }
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaggedUnion {
    pub enum_name: String,
    pub variants: Vec<Variant>,
    #[serde(default)]
    pub representation: EnumRepresentation,
    #[serde(default)]
    pub doccom: Option<String>,
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub pipeline: Pipeline,
    #[serde(default)]
    pub rust_derives: Vec<String>,
    #[serde(default)]
    pub output_override: Option<Box<Self>>,
    /// When true, this enum is registered for field-type resolution only and is
    /// skipped at every emission site (typesync interface output). Set for types
    /// from a `resolve_only` `include_files` entry.
    #[serde(default)]
    pub resolve_only: bool,
    #[serde(default)]
    pub raw_attributes: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    pub data: Option<VariantData>,
    #[serde(default)]
    pub wire: Wire,
    #[serde(default)]
    pub doccom: Option<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub output_override: Option<Box<Self>>,
    #[serde(default)]
    pub raw_attributes: BTreeMap<String, Vec<String>>,
    /// True for the variant marked `#[default]`, the same attribute
    /// `#[derive(Default)]` uses to pick an enum's default variant. Default
    /// synthesis (both SurrealDB and TypeScript) picks this variant; if no
    /// variant is flagged, the first declared variant is used.
    #[serde(default)]
    pub is_default: bool,
    /// A tuple variant's morphs, one list per element of its payload.
    #[serde(default)]
    pub element_morphs: Vec<Vec<Morph>>,
    /// A tuple variant's validators, one list per element of its payload.
    #[serde(default)]
    pub element_validators: Vec<Vec<Validator>>,
    /// Lists replacing each element's validators in one pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub element_validator_overrides: Vec<ValidatorOverrides>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantData {
    InlineStruct(StructConfig),
    DataStructureRef(FieldType),
}

impl VariantData {
    /// The type a link to this variant's enum reaches through it: a newtype
    /// variant's type, the field type of a struct variant with one field (as
    /// `EvenframeUnion` reads it), or another struct variant's own name.
    pub fn linked_type_name(&self) -> Option<&str> {
        match self {
            VariantData::DataStructureRef(FieldType::Other(name)) => Some(name),
            VariantData::DataStructureRef(_) => None,
            VariantData::InlineStruct(inline) => match inline.fields.as_slice() {
                [only] => match &only.field_type {
                    FieldType::Other(name) => Some(name),
                    _ => None,
                },
                _ => Some(&inline.struct_name),
            },
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructField {
    pub field_name: String,
    pub field_type: FieldType,
    #[serde(default)]
    pub wire: Wire,
    pub edge_config: Option<EdgeConfig>,
    pub define_config: Option<DefineConfig>,
    pub format: Option<MockFormat>,
    /// Rewrites the value into canonical form before `validators` check it.
    #[serde(default)]
    pub morphs: Vec<Morph>,
    pub validators: Vec<Validator>,
    /// Lists replacing `validators` in one pipeline.
    #[serde(default, skip_serializing_if = "ValidatorOverrides::is_empty")]
    pub validator_overrides: ValidatorOverrides,
    pub always_regenerate: bool,
    #[serde(default)]
    pub doccom: Option<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub output_override: Option<Box<Self>>,
    #[serde(default)]
    pub raw_attributes: BTreeMap<String, Vec<String>>,
}

impl StructField {
    pub fn unit(field_name: String) -> Self {
        Self {
            field_name,
            field_type: FieldType::Unit,
            ..Default::default()
        }
    }

    pub fn partial(field_name: &str) -> Self {
        Self {
            field_name: field_name.to_string(),
            field_type: FieldType::Struct(Vec::new()),
            ..Default::default()
        }
    }

    /// Resolve `output_override` recursively. Every consumer that reads a
    /// `StructField` should call this first: `output_override` is a literal
    /// replacement, applied uniformly across all consumers.
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }

    /// The field's key in serde's JSON.
    pub fn serde_name(&self) -> &str {
        self.wire.serde.as_deref().unwrap_or(&self.field_name)
    }

    /// The generated key, with camelCase as the fallback.
    pub fn ts_name(&self) -> std::borrow::Cow<'_, str> {
        match self
            .wire
            .typescript
            .as_deref()
            .or(self.wire.serde.as_deref())
        {
            Some(name) => std::borrow::Cow::Borrowed(name),
            None => std::borrow::Cow::Owned(self.field_name.to_case(Case::Camel)),
        }
    }

    /// The field's key in the database, as SurrealValue writes it.
    pub fn db_name(&self) -> &str {
        self.wire.surreal.as_deref().unwrap_or(&self.field_name)
    }

    /// Whether mock data writes a value for this field. The record id is set
    /// separately, edges live in their relation tables, and skipped, readonly
    /// and computed fields are not stored from input.
    pub fn is_mock_written(&self) -> bool {
        self.db_name() != "id"
            && self.edge_config.is_none()
            && self.define_config.as_ref().is_none_or(|define| {
                !define.should_skip
                    && !define.readonly.unwrap_or(false)
                    && define.computed.is_none()
            })
    }

    /// The field's `DEFINE FIELD` statements on `table_name`, recording each
    /// position whose validators the schema cannot assert in `unasserted`.
    #[cfg(feature = "schemadump")]
    pub fn generate_define_statement(
        &self,
        owner: &FieldOwner,
        table_name: &str,
        context: &crate::schemasync::database::surql::shape::DefineContext<'_>,
        unasserted: &mut Vec<crate::schemasync::database::surql::shape::Unasserted>,
    ) -> Result<String> {
        use crate::schemasync::database::surql::shape::{
            field_shape, own_assertion, own_zero_ok, type_surql,
        };
        let options = context.options;

        let mut stmt = format!(
            "DEFINE FIELD OVERWRITE {} ON TABLE {}",
            surql_ident(self.db_name()),
            table_name
        );

        // Handle computed fields (SurrealDB 3.0 COMPUTED syntax)
        if let Some(ref def) = self.define_config
            && let Some(ref computed_expr) = def.computed
        {
            stmt.push_str(&format!(" COMPUTED {}", computed_expr));

            // TYPE is optional for computed fields but include if explicitly set or auto-detected
            let type_str = if let Some(ref data_type) = def.data_type {
                data_type.clone()
            } else {
                type_surql(&self.field_type, self.db_name(), table_name, context)?
            };

            if def.flexible.unwrap_or(false) {
                stmt.push_str(" FLEXIBLE");
            }

            if !type_str.is_empty() {
                stmt.push_str(&format!(" TYPE {}", type_str));
            }

            // Permissions for computed fields (select/create/update only, no delete)
            let mut permissions = Vec::new();
            if let Some(ref perm) = def.select_permissions {
                permissions.push(format!("FOR select {}", perm));
            }
            if let Some(ref perm) = def.create_permissions {
                permissions.push(format!("FOR create {}", perm));
            }
            if let Some(ref perm) = def.update_permissions {
                permissions.push(format!("FOR update {}", perm));
            }
            if !permissions.is_empty() {
                stmt.push_str(&format!(" PERMISSIONS {}", permissions.join(" ")));
            }

            if let Some(ref comment_str) = def.comment {
                stmt.push_str(&format!(
                    " COMMENT {}",
                    crate::schemasync::table::surql_string_literal(comment_str)
                ));
            }

            stmt.push_str(";\n");
            return Ok(stmt);
        }

        // A field whose stored type is not taken from its Rust type asserts
        // its own validators alone.
        let (type_str, map_values, generated_assert, zero_ok) = match &self.define_config {
            Some(def) if def.should_skip => (
                String::new(),
                None,
                own_assertion(self, options),
                own_zero_ok(self),
            ),
            Some(DefineConfig {
                data_type: Some(data_type),
                ..
            }) => (
                data_type.clone(),
                None,
                own_assertion(self, options),
                own_zero_ok(self),
            ),
            _ => {
                let shape = field_shape(self, owner, table_name, context, unasserted)?;
                (
                    shape.surql,
                    shape.map_values,
                    shape.assertion,
                    shape.zero_ok,
                )
            }
        };

        // A struct with keys known only from a value is any object, which
        // keeps its keys only on a flexible field.
        let flexible = self
            .define_config
            .as_ref()
            .is_some_and(|def| def.flexible.unwrap_or(false))
            || holds_open_struct(
                &self.field_type,
                context.enums,
                context.objects,
                context.tables,
                &mut HashSet::new(),
            );
        if flexible {
            stmt.push_str(" FLEXIBLE");
        }

        if !type_str.is_empty() {
            stmt.push_str(&format!(" TYPE {}", type_str));
        }

        let nullable_option = matches!(self.field_type, FieldType::Option(_))
            && options.option_none == crate::schemasync::config::OptionNone::Null;
        if let Some(ref def) = self.define_config {
            if let Some(ref def_val) = def.default {
                let always = if def.default_always.is_some() {
                    " ALWAYS"
                } else {
                    ""
                };
                stmt.push_str(&format!(" DEFAULT{} {}", always, def_val));
            } else if nullable_option {
                stmt.push_str(" DEFAULT NULL");
            } else if !matches!(self.field_type, FieldType::Option(_))
                && zero_ok
                && let Some(default) = crate::default::field_type_to_surql_default(
                    &self.field_name,
                    table_name,
                    &self.field_type,
                    context.enums,
                    context.objects,
                    crate::schemasync::config::SurqlContext {
                        registry: context.registry,
                        options,
                    },
                )
            {
                stmt.push_str(&format!(" DEFAULT {default}"));
            }
            // Otherwise an optional field uses the configured absence value; no zero
            // value exists or it would violate the field's validators, so the
            // field is required instead of unsatisfiable.

            if def.readonly.unwrap_or(false) {
                stmt.push_str(" READONLY");
            }

            if let Some(ref val) = def.value {
                stmt.push_str(&format!(" VALUE {}", val));
            }

            // A hand-written clause is kept verbatim when it is the only part;
            // combined, each part is parenthesized to keep precedence.
            let manual = def
                .assert
                .as_deref()
                .map(str::trim)
                .filter(|manual| !manual.is_empty());
            let assert_clause = match (manual, generated_assert) {
                (Some(manual), None) => Some(manual.to_owned()),
                (None, Some(generated)) => Some(generated),
                (Some(manual), Some(generated)) => Some(format!("({manual}) AND ({generated})")),
                (None, None) => None,
            };
            if let Some(assert_clause) = assert_clause {
                stmt.push_str(&format!(" ASSERT {}", assert_clause));
            }
        } else if nullable_option {
            stmt.push_str(" DEFAULT NULL");
        }

        if let Some(ref def) = self.define_config {
            let mut permissions = Vec::new();

            if let Some(ref perm) = def.select_permissions {
                permissions.push(format!("FOR select {}", perm));
            }
            if let Some(ref perm) = def.create_permissions {
                permissions.push(format!("FOR create {}", perm));
            }
            if let Some(ref perm) = def.update_permissions {
                permissions.push(format!("FOR update {}", perm));
            }

            if !permissions.is_empty() {
                stmt.push_str(&format!(" PERMISSIONS {}", permissions.join(" ")));
            }

            if let Some(ref comment_str) = def.comment {
                stmt.push_str(&format!(
                    " COMMENT {}",
                    crate::schemasync::table::surql_string_literal(comment_str)
                ));
            }
        }

        stmt.push_str(";\n");

        if let Some(wildcard_value_type) = map_values {
            stmt.push_str(&format!(
                "DEFINE FIELD OVERWRITE {}.* ON TABLE {} TYPE {};\n",
                surql_ident(self.db_name()),
                table_name,
                wildcard_value_type
            ));
        }

        Ok(stmt)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructConfig {
    pub struct_name: String,
    pub fields: Vec<StructField>,
    #[serde(default)]
    pub doccom: Option<String>,
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub pipeline: Pipeline,
    #[serde(default)]
    pub rust_derives: Vec<String>,
    #[serde(default)]
    pub output_override: Option<Box<Self>>,
    /// When true, this struct is registered for field-type resolution only and
    /// is skipped at every emission site (schemasync `DEFINE TABLE`/mock/diff,
    /// typesync interface output). Set for types from a `resolve_only`
    /// `include_files` entry.
    #[serde(default)]
    pub resolve_only: bool,
    /// Every attribute on this item that evenframe doesn't natively parse,
    /// keyed by attribute name. Each value is one occurrence's raw token
    /// body (the part inside the parens). Multiple uses of the same
    /// attribute produce multiple entries under that key.
    ///
    /// Synthetic plugins can read project-specific attributes like
    /// `#[partial_route(name = "PartialUser", fields(id, email))]`
    /// without evenframe needing to understand them.
    #[serde(default)]
    pub raw_attributes: BTreeMap<String, Vec<String>>,
}

/// Whether a value of `field_type` holds, below any link to a table, a struct
/// with keys known only from a value, which the schema stores as any object.
#[cfg(feature = "schemadump")]
fn holds_open_struct(
    field_type: &FieldType,
    enums: &BTreeMap<String, TaggedUnion>,
    app_structs: &BTreeMap<String, StructConfig>,
    persistable_structs: &BTreeMap<String, TableConfig>,
    visited: &mut HashSet<String>,
) -> bool {
    let holds = |held: &FieldType, visited: &mut HashSet<String>| {
        holds_open_struct(held, enums, app_structs, persistable_structs, visited)
    };
    match field_type {
        FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::JsonText(inner) => {
            holds(inner, visited)
        }
        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            holds(key, visited) || holds(value, visited)
        }
        FieldType::Tuple(items) => items.iter().any(|item| holds(item, visited)),
        FieldType::Struct(members) => members.iter().any(|(_, member)| holds(member, visited)),
        FieldType::Other(name) => {
            if !visited.insert(name.clone()) {
                return false;
            }
            // A table held by value is stored as a link to its record, as the
            // schema's types write it.
            if let Some(app_struct) = app_structs.get(name).map(StructConfig::effective) {
                if persistable_structs.contains_key(&app_struct.struct_name.to_case(Case::Snake)) {
                    return false;
                }
                return app_struct.is_open()
                    || app_struct
                        .fields
                        .iter()
                        .any(|field| holds(&field.effective().field_type, visited));
            }
            if persistable_structs.contains_key(&name.to_case(Case::Snake)) {
                return false;
            }
            enums.get(name).is_some_and(|tagged_union| {
                tagged_union.effective().variants.iter().any(|variant| {
                    match &variant.effective().data {
                        Some(VariantData::InlineStruct(inline)) => inline
                            .effective()
                            .fields
                            .iter()
                            .any(|field| holds(&field.effective().field_type, visited)),
                        Some(VariantData::DataStructureRef(held)) => holds(held, visited),
                        None => false,
                    }
                })
            })
        }
        FieldType::RecordLink(_)
        | FieldType::String
        | FieldType::Char
        | FieldType::Bool
        | FieldType::Unit
        | FieldType::F32
        | FieldType::F64
        | FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize
        | FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize
        | FieldType::Duration
        | FieldType::FromText(_)
        | FieldType::IsoDate
        | FieldType::EpochMillis => false,
    }
}

impl StructConfig {
    /// Whether the database holds keys beside the struct's own that are known
    /// only from a value: those of a flattened map or enum. A flattened
    /// struct's fields are put in its place by the pipeline views, so only
    /// these remain flattened.
    pub fn is_open(&self) -> bool {
        self.effective()
            .fields
            .iter()
            .any(|field| field.effective().wire.storage.flatten)
    }

    /// Resolve `output_override` recursively. Every consumer that reads a
    /// `StructConfig` should call this first: `output_override` is a literal
    /// replacement, applied uniformly across all consumers.
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }
}

impl TaggedUnion {
    /// Resolve `output_override` recursively. See [`StructConfig::effective`].
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }
}

impl Variant {
    /// Resolve `output_override` recursively. See [`StructConfig::effective`].
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }

    /// How the database stores this variant: as its enum's `#[surreal]`
    /// representation says, else as serde writes it.
    pub fn stored_representation<'a>(
        &'a self,
        enum_representation: &'a EnumRepresentation,
    ) -> &'a EnumRepresentation {
        self.wire
            .storage
            .representation
            .as_ref()
            .unwrap_or_else(|| self.serde_representation(enum_representation))
    }

    /// How serde writes this variant: bare under its own
    /// `#[serde(untagged)]`, else as its enum writes every variant.
    pub fn serde_representation<'a>(
        &self,
        enum_representation: &'a EnumRepresentation,
    ) -> &'a EnumRepresentation {
        if self.wire.serde_untagged {
            &EnumRepresentation::Untagged
        } else {
            enum_representation
        }
    }

    /// The variant's name in serde's JSON and the generated TypeScript.
    pub fn serde_name(&self) -> &str {
        self.wire.serde.as_deref().unwrap_or(&self.name)
    }

    /// The variant's name in the database, as SurrealValue writes it.
    pub fn db_name(&self) -> &str {
        self.wire.surreal.as_deref().unwrap_or(&self.name)
    }
}

/// The keys (in `tables`) of the tables a `RecordLink<type_name>` can point
/// at: the type's own table, the table a projection object's
/// `output_override` stands for, or every table variant of a persistable
/// union. Empty when `type_name` names none of those.
#[cfg(feature = "schemadump")]
pub fn link_target_tables(
    type_name: &str,
    tables: &BTreeMap<String, TableConfig>,
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
) -> Vec<String> {
    let table_key = |name: &str| {
        let key = name.to_case(Case::Snake);
        tables.contains_key(&key).then_some(key)
    };
    if let Some(key) = table_key(type_name) {
        return vec![key];
    }
    if let Some(key) = structs
        .get(type_name)
        .and_then(|s| table_key(&s.effective().struct_name))
    {
        return vec![key];
    }
    enums
        .get(type_name)
        .map(|union| {
            union
                .variants
                .iter()
                .filter_map(|variant| table_key(variant.data.as_ref()?.linked_type_name()?))
                .collect()
        })
        .unwrap_or_default()
}

/// How a `RecordLink<type_name>` names its target in SurrealQL: every table
/// it can point at, joined with ` | `, or else the table a projection
/// object's `output_override` names. `None` when `type_name` is neither.
#[cfg(feature = "schemadump")]
pub fn record_link_target_surql(
    type_name: &str,
    tables: &BTreeMap<String, TableConfig>,
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
) -> Option<String> {
    let names: Vec<String> = link_target_tables(type_name, tables, structs, enums)
        .iter()
        .filter_map(|key| tables.get(key))
        .map(|table| table.effective().table_name.clone())
        .collect();
    if !names.is_empty() {
        return Some(names.join(" | "));
    }
    structs
        .get(type_name)
        .map(|s| s.effective().struct_name.to_case(Case::Snake))
}

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, EnumRepresentation, FieldType, Pipeline, StructConfig, StructField, TaggedUnion,
        Variant, VariantData,
    };
    #[cfg(feature = "schemadump")]
    use super::{ForeignTypeRegistry, TableConfig};

    // ==================== TaggedUnion Tests ====================

    #[test]
    fn test_tagged_union_equality() {
        let tu1 = TaggedUnion {
            resolve_only: false,
            enum_name: "Status".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let tu2 = TaggedUnion {
            resolve_only: false,
            enum_name: "Status".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert_eq!(tu1, tu2);
    }

    #[test]
    fn test_tagged_union_with_variants() {
        let tu = TaggedUnion {
            resolve_only: false,
            enum_name: "Status".to_string(),
            variants: vec![
                Variant {
                    name: "Active".to_string(),
                    wire: Default::default(),
                    data: None,
                    doccom: None,
                    annotations: vec![],
                    output_override: None,
                    raw_attributes: BTreeMap::new(),
                    is_default: false,
                    element_morphs: Vec::new(),
                    element_validators: Vec::new(),
                    element_validator_overrides: Vec::new(),
                },
                Variant {
                    name: "Inactive".to_string(),
                    wire: Default::default(),
                    data: None,
                    doccom: None,
                    annotations: vec![],
                    output_override: None,
                    raw_attributes: BTreeMap::new(),
                    is_default: false,
                    element_morphs: Vec::new(),
                    element_validators: Vec::new(),
                    element_validator_overrides: Vec::new(),
                },
            ],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert_eq!(tu.variants.len(), 2);
        assert_eq!(tu.variants[0].name, "Active");
    }

    #[test]
    fn test_tagged_union_serialize_deserialize() {
        let tu = TaggedUnion {
            resolve_only: false,
            enum_name: "Color".to_string(),
            variants: vec![Variant {
                name: "Red".to_string(),
                wire: Default::default(),
                data: None,
                doccom: None,
                annotations: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
                is_default: false,
                element_morphs: Vec::new(),
                element_validators: Vec::new(),
                element_validator_overrides: Vec::new(),
            }],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&tu).unwrap();
        let deserialized: TaggedUnion = serde_json::from_str(&json).unwrap();
        assert_eq!(tu, deserialized);
    }

    #[test]
    fn test_tagged_union_distinctness() {
        let tu1 = TaggedUnion {
            resolve_only: false,
            enum_name: "A".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let tu2 = TaggedUnion {
            resolve_only: false,
            enum_name: "B".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert_ne!(tu1, tu2);
        let items = [tu1, tu2];
        assert_eq!(items.len(), 2);
        assert_ne!(items[0], items[1]);
    }

    // ==================== Variant Tests ====================

    #[test]
    fn test_variant_unit() {
        let v = Variant {
            name: "None".to_string(),
            wire: Default::default(),
            data: None,
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default: false,
            element_morphs: Vec::new(),
            element_validators: Vec::new(),
            element_validator_overrides: Vec::new(),
        };
        assert!(v.data.is_none());
    }

    #[test]
    fn test_variant_with_data_structure_ref() {
        let v = Variant {
            name: "Some".to_string(),
            wire: Default::default(),
            data: Some(VariantData::DataStructureRef(FieldType::String)),
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default: false,
            element_morphs: Vec::new(),
            element_validators: Vec::new(),
            element_validator_overrides: Vec::new(),
        };
        assert!(matches!(
            v.data,
            Some(VariantData::DataStructureRef(FieldType::String))
        ));
    }

    #[test]
    fn test_variant_with_inline_struct() {
        let struct_config = StructConfig {
            resolve_only: false,
            struct_name: "InnerData".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let v = Variant {
            name: "Complex".to_string(),
            wire: Default::default(),
            data: Some(VariantData::InlineStruct(struct_config)),
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default: false,
            element_morphs: Vec::new(),
            element_validators: Vec::new(),
            element_validator_overrides: Vec::new(),
        };
        assert!(matches!(v.data, Some(VariantData::InlineStruct(_))));
    }

    // ==================== VariantData Tests ====================

    #[test]
    fn test_variant_data_equality() {
        let vd1 = VariantData::DataStructureRef(FieldType::I32);
        let vd2 = VariantData::DataStructureRef(FieldType::I32);
        assert_eq!(vd1, vd2);
    }

    #[test]
    fn test_variant_data_inline_struct_vs_ref() {
        let vd1 = VariantData::DataStructureRef(FieldType::String);
        let vd2 = VariantData::InlineStruct(StructConfig {
            resolve_only: false,
            struct_name: "Test".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        });
        assert_ne!(vd1, vd2);
    }

    #[test]
    fn a_variant_links_through_its_newtype_or_only_field() {
        let field = |name: &str, field_type: FieldType| StructField {
            field_name: name.to_string(),
            field_type,
            ..Default::default()
        };
        let inline = |fields: Vec<StructField>| {
            VariantData::InlineStruct(StructConfig {
                struct_name: "Named".to_string(),
                fields,
                ..Default::default()
            })
        };
        let invoice = || FieldType::Other("Invoice".to_string());
        assert_eq!(
            VariantData::DataStructureRef(invoice()).linked_type_name(),
            Some("Invoice")
        );
        assert_eq!(
            VariantData::DataStructureRef(FieldType::U32).linked_type_name(),
            None
        );
        assert_eq!(
            inline(vec![field("invoice", invoice())]).linked_type_name(),
            Some("Invoice")
        );
        assert_eq!(
            inline(vec![field("count", FieldType::U32)]).linked_type_name(),
            None
        );
        assert_eq!(
            inline(vec![
                field("invoice", invoice()),
                field("note", FieldType::String)
            ])
            .linked_type_name(),
            Some("Named")
        );
    }

    // ==================== StructField Tests ====================

    #[test]
    fn test_struct_field_unit() {
        let field = StructField::unit("name".to_string());
        assert_eq!(field.field_name, "name");
        assert!(matches!(field.field_type, FieldType::Unit));
    }

    #[test]
    fn test_struct_field_partial() {
        let field = StructField::partial("data");
        assert_eq!(field.field_name, "data");
        assert!(matches!(field.field_type, FieldType::Struct(_)));
    }

    #[test]
    fn test_struct_field_default() {
        let field = StructField::default();
        assert!(field.field_name.is_empty());
        assert!(field.validators.is_empty());
    }

    #[test]
    fn test_struct_field_equality() {
        let f1 = StructField {
            field_name: "id".to_string(),
            wire: Default::default(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: None,
            format: None,
            morphs: Vec::new(),
            validators: vec![],
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };
        let f2 = f1.clone();
        assert_eq!(f1, f2);
    }

    // ==================== StructConfig Tests ====================

    #[test]
    fn test_struct_config_empty() {
        let sc = StructConfig {
            resolve_only: false,
            struct_name: "Empty".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert!(sc.fields.is_empty());
    }

    #[test]
    fn test_struct_config_with_fields() {
        let sc = StructConfig {
            resolve_only: false,
            struct_name: "User".to_string(),
            fields: vec![
                StructField {
                    field_name: "id".to_string(),
                    wire: Default::default(),
                    field_type: FieldType::String,
                    edge_config: None,
                    define_config: None,
                    format: None,
                    morphs: Vec::new(),
                    validators: vec![],
                    always_regenerate: false,
                    doccom: None,
                    annotations: vec![],
                    unique: false,
                    output_override: None,
                    raw_attributes: BTreeMap::new(),
                    validator_overrides: Default::default(),
                },
                StructField {
                    field_name: "age".to_string(),
                    wire: Default::default(),
                    field_type: FieldType::I32,
                    edge_config: None,
                    define_config: None,
                    format: None,
                    morphs: Vec::new(),
                    validators: vec![],
                    always_regenerate: false,
                    doccom: None,
                    annotations: vec![],
                    unique: false,
                    output_override: None,
                    raw_attributes: BTreeMap::new(),
                    validator_overrides: Default::default(),
                },
            ],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert_eq!(sc.fields.len(), 2);
    }

    #[test]
    fn test_struct_config_serialize_deserialize() {
        let sc = StructConfig {
            resolve_only: false,
            struct_name: "Test".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let json = serde_json::to_string(&sc).unwrap();
        let deserialized: StructConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(sc, deserialized);
    }

    // ==================== FieldType Basic Tests ====================

    #[test]
    fn test_field_type_primitives() {
        assert!(matches!(FieldType::String, FieldType::String));
        assert!(matches!(FieldType::I32, FieldType::I32));
        assert!(matches!(FieldType::Bool, FieldType::Bool));
        assert!(matches!(FieldType::F64, FieldType::F64));
    }

    #[test]
    fn test_field_type_option() {
        let ft = FieldType::Option(Box::new(FieldType::String));
        assert!(matches!(ft, FieldType::Option(_)));
    }

    #[test]
    fn test_field_type_vec() {
        let ft = FieldType::Vec(Box::new(FieldType::I32));
        assert!(matches!(ft, FieldType::Vec(_)));
    }

    #[test]
    fn test_field_type_tuple() {
        let ft = FieldType::Tuple(vec![FieldType::String, FieldType::I32]);
        assert!(matches!(ft, FieldType::Tuple(_)));
    }

    #[test]
    fn test_field_type_other() {
        let ft = FieldType::Other("CustomType".to_string());
        assert!(matches!(ft, FieldType::Other(ref s) if s == "CustomType"));
    }

    #[test]
    fn test_field_type_hashmap() {
        let ft = FieldType::HashMap(Box::new(FieldType::String), Box::new(FieldType::I32));
        assert!(matches!(ft, FieldType::HashMap(_, _)));
    }

    #[test]
    fn test_field_type_btreemap() {
        let ft = FieldType::BTreeMap(Box::new(FieldType::String), Box::new(FieldType::Bool));
        assert!(matches!(ft, FieldType::BTreeMap(_, _)));
    }

    #[test]
    fn test_field_type_record_link() {
        let ft = FieldType::RecordLink(Box::new(FieldType::Other("User".to_string())));
        assert!(matches!(ft, FieldType::RecordLink(_)));
    }

    #[test]
    fn test_field_type_struct_inline() {
        let ft = FieldType::Struct(vec![
            ("name".to_string(), FieldType::String),
            ("value".to_string(), FieldType::I32),
        ]);
        assert!(matches!(ft, FieldType::Struct(_)));
    }

    // ==================== FieldType Equality Tests ====================

    #[test]
    fn test_field_type_equality_primitives() {
        assert_eq!(FieldType::String, FieldType::String);
        assert_ne!(FieldType::String, FieldType::I32);
    }

    #[test]
    fn test_field_type_equality_nested() {
        let ft1 = FieldType::Option(Box::new(FieldType::String));
        let ft2 = FieldType::Option(Box::new(FieldType::String));
        let ft3 = FieldType::Option(Box::new(FieldType::I32));
        assert_eq!(ft1, ft2);
        assert_ne!(ft1, ft3);
    }

    // ==================== FieldType Hash Tests ====================

    #[test]
    fn test_field_type_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(FieldType::String);
        set.insert(FieldType::I32);
        set.insert(FieldType::String); // duplicate
        assert_eq!(set.len(), 2);
    }

    // ==================== FieldType Clone Tests ====================

    #[test]
    fn test_field_type_clone() {
        let ft = FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::I32))));
        let cloned = ft.clone();
        assert_eq!(ft, cloned);
    }

    // ==================== FieldType Debug Tests ====================

    #[test]
    fn test_field_type_debug() {
        let ft = FieldType::String;
        let debug = format!("{:?}", ft);
        assert!(debug.contains("String"));
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_struct_config() {
        let sc = StructConfig {
            resolve_only: false,
            struct_name: "".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        assert!(sc.struct_name.is_empty());
    }

    #[test]
    fn test_deeply_nested_field_type() {
        let ft = FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::HashMap(
            Box::new(FieldType::String),
            Box::new(FieldType::Option(Box::new(FieldType::I32))),
        )))));
        assert!(matches!(ft, FieldType::Option(_)));
    }

    #[test]
    fn test_struct_field_with_validators() {
        use crate::validator::{StringValidator, Validator};
        let field = StructField {
            field_name: "email".to_string(),
            wire: Default::default(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: None,
            format: None,
            morphs: Vec::new(),
            validators: vec![Validator::StringValidator(StringValidator::Email)],
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };
        assert_eq!(field.validators.len(), 1);
    }

    // ==================== effective() Override Resolution Tests ====================

    #[test]
    fn test_struct_config_effective_returns_self_when_no_override() {
        let sc = StructConfig {
            struct_name: "User".to_string(),
            ..StructConfig::default()
        };
        assert_eq!(sc.effective().struct_name, "User");
    }

    #[test]
    fn test_struct_config_effective_returns_override() {
        let user = StructConfig {
            struct_name: "User".to_string(),
            ..StructConfig::default()
        };
        let partial_user = StructConfig {
            struct_name: "PartialUser".to_string(),
            output_override: Some(Box::new(user)),
            ..StructConfig::default()
        };
        assert_eq!(partial_user.effective().struct_name, "User");
    }

    #[test]
    fn test_struct_config_effective_resolves_chained_overrides() {
        let target = StructConfig {
            struct_name: "Target".to_string(),
            ..StructConfig::default()
        };
        let middle = StructConfig {
            struct_name: "Middle".to_string(),
            output_override: Some(Box::new(target)),
            ..StructConfig::default()
        };
        let head = StructConfig {
            struct_name: "Head".to_string(),
            output_override: Some(Box::new(middle)),
            ..StructConfig::default()
        };
        assert_eq!(head.effective().struct_name, "Target");
    }

    #[test]
    fn test_struct_field_effective_returns_override() {
        let real = StructField {
            field_name: "real".to_string(),
            field_type: FieldType::I32,
            ..StructField::default()
        };
        let aliased = StructField {
            field_name: "aliased".to_string(),
            field_type: FieldType::String,
            output_override: Some(Box::new(real)),
            ..StructField::default()
        };
        assert_eq!(aliased.effective().field_name, "real");
        assert_eq!(aliased.effective().field_type, FieldType::I32);
    }

    #[test]
    fn test_tagged_union_effective_returns_override() {
        let real_enum = TaggedUnion {
            resolve_only: false,
            enum_name: "Real".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let aliased = TaggedUnion {
            resolve_only: false,
            enum_name: "Aliased".to_string(),
            variants: vec![],
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::default(),
            rust_derives: vec![],
            output_override: Some(Box::new(real_enum)),
            raw_attributes: BTreeMap::new(),
        };
        assert_eq!(aliased.effective().enum_name, "Real");
    }

    #[test]
    fn test_variant_effective_returns_override() {
        let real_variant = Variant {
            name: "Real".to_string(),
            wire: Default::default(),
            data: None,
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default: false,
            element_morphs: Vec::new(),
            element_validators: Vec::new(),
            element_validator_overrides: Vec::new(),
        };
        let aliased = Variant {
            name: "Aliased".to_string(),
            wire: Default::default(),
            data: None,
            doccom: None,
            annotations: vec![],
            output_override: Some(Box::new(real_variant)),
            raw_attributes: BTreeMap::new(),
            is_default: false,
            element_morphs: Vec::new(),
            element_validators: Vec::new(),
            element_validator_overrides: Vec::new(),
        };
        assert_eq!(aliased.effective().name, "Real");
    }

    #[cfg(feature = "schemadump")]
    #[test]
    fn test_generate_define_statement_resolves_record_link_override() {
        // Models a synthetic plugin that adds a projection:
        // a real table field is `Vec<RecordLink<PartialUser>>`, where
        // `PartialUser` is a TS-only projection whose `output_override`
        // points back to the underlying `User` table.
        let field = StructField {
            field_name: "dm_participants".to_string(),
            wire: Default::default(),
            field_type: FieldType::Vec(Box::new(FieldType::RecordLink(Box::new(
                FieldType::Other("PartialUser".to_string()),
            )))),
            edge_config: None,
            define_config: Some(crate::schemasync::DefineConfig {
                select_permissions: Some("FULL".to_string()),
                update_permissions: Some("FULL".to_string()),
                create_permissions: Some("FULL".to_string()),
                data_type: None,
                should_skip: false,
                default: None,
                default_always: None,
                value: None,
                assert: None,
                readonly: None,
                flexible: Some(false),
                computed: None,
                comment: None,
            }),
            format: None,
            morphs: Vec::new(),
            validators: vec![],
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        // Synthetic struct: PartialUser overrides to User
        let partial_user = StructConfig {
            resolve_only: false,
            struct_name: "PartialUser".to_string(),
            fields: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::Typesync,
            rust_derives: vec![],
            output_override: Some(Box::new(StructConfig {
                struct_name: "User".to_string(),
                ..StructConfig::default()
            })),
            raw_attributes: BTreeMap::new(),
        };

        // Real `user` table, the override target
        let user_table = TableConfig {
            table_name: "user".to_string(),
            struct_config: StructConfig {
                struct_name: "User".to_string(),
                ..StructConfig::default()
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![],
            output_override: None,
        };

        let mut app_structs = BTreeMap::new();
        app_structs.insert("PartialUser".to_string(), partial_user);

        let mut tables = BTreeMap::new();
        tables.insert("user".to_string(), user_table);

        let stmt = field
            .generate_define_statement(
                &crate::types::FieldOwner::Table("errand_channel".to_string()),
                "errand_channel",
                &crate::schemasync::database::surql::shape::DefineContext {
                    tables: &tables,
                    objects: &app_structs,
                    enums: &BTreeMap::new(),
                    declared: &crate::types::DeclaredTypes::default(),
                    registry: &ForeignTypeRegistry::default(),
                    options: crate::schemasync::config::SurqlOptions::from(true),
                },
                &mut Vec::new(),
            )
            .expect("generate_define_statement should succeed");

        assert!(
            stmt.contains("TYPE array<record<user>>"),
            "expected `array<record<user>>` after override resolution; got: {stmt}"
        );
        assert!(
            !stmt.contains("partial_user"),
            "PartialUser must not leak into the DEFINE FIELD output: {stmt}"
        );
    }

    #[cfg(feature = "schemadump")]
    #[test]
    fn test_generate_define_statement_no_override_emits_literal_name() {
        // Regression guard: without `output_override`, the historical
        // behavior (emitting the type name verbatim, snake-cased) must
        // hold. If this test ever flips, schemasync would silently rename
        // every record link.
        let field = StructField {
            field_name: "dm_participants".to_string(),
            wire: Default::default(),
            field_type: FieldType::Vec(Box::new(FieldType::RecordLink(Box::new(
                FieldType::Other("PartialUser".to_string()),
            )))),
            edge_config: None,
            define_config: Some(crate::schemasync::DefineConfig {
                select_permissions: Some("FULL".to_string()),
                update_permissions: Some("FULL".to_string()),
                create_permissions: Some("FULL".to_string()),
                data_type: None,
                should_skip: false,
                default: None,
                default_always: None,
                value: None,
                assert: None,
                readonly: None,
                flexible: Some(false),
                computed: None,
                comment: None,
            }),
            format: None,
            morphs: Vec::new(),
            validators: vec![],
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let stmt = field
            .generate_define_statement(
                &crate::types::FieldOwner::Table("errand_channel".to_string()),
                "errand_channel",
                &crate::schemasync::database::surql::shape::DefineContext {
                    tables: &BTreeMap::new(),
                    objects: &BTreeMap::new(),
                    enums: &BTreeMap::new(),
                    declared: &crate::types::DeclaredTypes::default(),
                    registry: &ForeignTypeRegistry::default(),
                    options: crate::schemasync::config::SurqlOptions::from(true),
                },
                &mut Vec::new(),
            )
            .expect("generate_define_statement should succeed");

        assert!(
            stmt.contains("TYPE array<record<partial_user>>"),
            "without override, expected literal `record<partial_user>`; got: {stmt}"
        );
    }
}
