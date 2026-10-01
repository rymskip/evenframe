//! Macroforge TypeScript interface generation with JSDoc validator annotations.
//!
//! This module generates TypeScript interfaces with `@derive(Deserialize)` at the type level
//! and `@serde({ validate: [...] })` annotations at the field level for validators.

use crate::config::{RECORD_LINK, fill};
use crate::error::{EvenframeError, Result};
use crate::types::{EnumRepresentation, FieldType, StructConfig, TaggedUnion, VariantData};
use crate::typesync::config::ArrayStyle;
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{Reading, foreign_types_used, import_lines};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::typesync::type_index::TypeIndex;
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, BTreeSet};

/// Pick the StructConfig view to emit fields and metadata from.
///
/// `output_override` has two distinct producers in the wild:
///
/// 1. Rule-plugin overrides preserve the struct name and only carry extra
///    metadata (annotations, derives). `effective()` returns a same-named
///    config and we want its content for the emitted interface.
/// 2. Synthetic projections (partials) redirect to a different struct
///    entirely. `effective()` returns the parent, but the partial still
///    needs its own TS interface with its own fields. Following the
///    redirect would emit the parent's content under the partial's name.
///
/// Use `effective()` only when the redirect preserves the struct name.
fn struct_view(struct_config: &StructConfig) -> &StructConfig {
    let effective = struct_config.effective();
    if effective.struct_name == struct_config.struct_name {
        effective
    } else {
        struct_config
    }
}

fn enum_view(enum_def: &TaggedUnion) -> &TaggedUnion {
    let effective = enum_def.effective();
    if effective.enum_name == enum_def.enum_name {
        effective
    } else {
        enum_def
    }
}

/// Main entry point for generating Macroforge TypeScript interfaces.
pub fn generate_macroforge_type_string(
    index: &TypeIndex,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    tracing::info!(
        struct_count = index.structs().len(),
        enum_count = index.enums().len(),
        "Generating Macroforge TypeScript interfaces"
    );

    // Each entry is unique by its own name. Synthetic projections (partials
    // whose `output_override` redirects to a different parent struct) are
    // kept as separate entries so they get their own TS interface.
    // `resolve_only` types are registered for resolution but not emitted as
    // their own interface (the owning run emits them / the consumer imports).
    let mut unique_structs: Vec<&(String, &StructConfig)> = index
        .named_structs()
        .iter()
        .filter(|(_, struct_config)| !struct_config.resolve_only)
        .collect();
    unique_structs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut unique_enums: Vec<&(String, &TaggedUnion)> = index
        .named_enums()
        .iter()
        .filter(|(_, tagged_union)| !tagged_union.resolve_only)
        .collect();
    unique_enums.sort_by(|left, right| left.0.cmp(&right.0));

    let all_type_names: Vec<String> = unique_structs
        .iter()
        .map(|(name, _)| name.clone())
        .chain(unique_enums.iter().map(|(name, _)| name.clone()))
        .collect();

    let mut result = String::new();
    let extra_imports = compute_extra_imports(&all_type_names, index, registry);
    if !extra_imports.lines.is_empty() {
        result.push_str(&extra_imports.lines.join("\n"));
        result.push_str("\n\n");
    }
    if extra_imports.needs_record_link {
        result.push_str(RECORD_LINK_TYPE);
        result.push_str("\n\n");
    }

    let mut parts: Vec<String> = Vec::new();
    for (_, struct_config) in &unique_structs {
        parts.push(generate_struct_block(struct_config, array_style, registry));
    }
    for (_, enum_def) in &unique_enums {
        parts.push(generate_enum_block(enum_def, array_style, registry));
    }

    result.push_str(&parts.join("\n"));
    tracing::info!(
        output_length = result.len(),
        "Macroforge interface generation complete"
    );
    result
}

/// A record link's TypeScript type: the linked record's id, or the record.
pub const RECORD_LINK_TYPE: &str = "export type RecordLink<T> = string | T;";

/// Imports a set of types needs beyond each other, and whether evenframe must
/// declare `RecordLink` for them: they use it and the project configures none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraImports {
    pub lines: Vec<String>,
    pub needs_record_link: bool,
}

/// Generates Macroforge TypeScript interfaces for a specific subset of types (used in per-file mode).
pub fn generate_macroforge_for_types(
    type_names: &[String],
    index: &TypeIndex,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let type_set: BTreeSet<&str> = type_names.iter().map(String::as_str).collect();

    // Filter to requested types by the entry's own name. See the full-output
    // path above for the rationale on not deduping by `effective()`.
    let mut filtered_structs: Vec<&(String, &StructConfig)> = index
        .named_structs()
        .iter()
        .filter(|(name, _)| type_set.contains(name.as_str()))
        .collect();
    filtered_structs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut filtered_enums: Vec<&(String, &TaggedUnion)> = index
        .named_enums()
        .iter()
        .filter(|(name, _)| type_set.contains(name.as_str()))
        .collect();
    filtered_enums.sort_by(|left, right| left.0.cmp(&right.0));

    let mut parts: Vec<String> = Vec::new();
    for (_, struct_config) in &filtered_structs {
        parts.push(generate_struct_block(struct_config, array_style, registry));
    }
    for (_, enum_def) in &filtered_enums {
        parts.push(generate_enum_block(enum_def, array_style, registry));
    }
    parts.join("\n")
}

/// Generate a single struct's TypeScript interface block.
fn generate_struct_block(
    struct_config: &StructConfig,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    // Always emit the interface under the entry's own struct_name; pull
    // body content (fields, derives, annotations) from `struct_view` so a
    // same-name override (rule plugin) is honored while a redirect
    // override (synthetic projection) leaves the partial's own body intact.
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let view = struct_view(struct_config);
    let mut lines: Vec<String> = Vec::new();

    let derive_line = format_derive_line(&view.macroforge_derives);
    if let Some(ref desc) = view.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.push(derive_line);
    for ann in &view.annotations {
        lines.push(format!("/** {} */", ann));
    }
    lines.push(format!("export interface {} {{", name));
    for field in &view.fields {
        lines.push(render_field_block(field, array_style, registry));
    }
    lines.push("}".to_string());
    lines.push(String::new());
    lines.join("\n")
}

/// Generate a single enum's TypeScript type block.
fn generate_enum_block(
    enum_def: &TaggedUnion,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    // Same approach as [`generate_struct_block`]: emit under the entry's own
    // enum_name, pull body from `enum_view` so same-name overrides are
    // applied while redirect overrides leave the entry's own body intact.
    let name = enum_def.enum_name.to_case(Case::Pascal);
    let view = enum_view(enum_def);
    let mut lines: Vec<String> = Vec::new();

    let derive_line = format_derive_line(&view.macroforge_derives);
    if let Some(ref desc) = view.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.push(derive_line);
    for ann in &view.annotations {
        lines.push(format!("/** {} */", ann));
    }

    // Emit @serde annotation for tagged representations so the macroforge
    // type registry knows how to parse/stringify these unions at runtime.
    match &view.representation {
        EnumRepresentation::InternallyTagged { tag } => {
            lines.push(format!("/** @serde({{ tag: \"{}\" }}) */", tag));
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            lines.push(format!(
                "/** @serde({{ tag: \"{}\", content: \"{}\" }}) */",
                tag, content
            ));
        }
        EnumRepresentation::ExternallyTagged => {
            lines.push("/** @serde({ externallyTagged: true }) */".to_string());
        }
        EnumRepresentation::Untagged => {
            lines.push("/** @serde({ untagged: true }) */".to_string());
        }
    }

    let variant_parts: Vec<String> = view
        .variants
        .iter()
        .map(|variant| render_variant(variant, &view.representation, array_style, registry))
        .collect();

    lines.push(format!(
        "export type {} =\n\t{};",
        name,
        variant_parts
            .iter()
            .map(|part| format!("| {}", part))
            .collect::<Vec<_>>()
            .join("\n\t")
    ));
    lines.push(String::new());
    lines.join("\n")
}

/// Render a single enum variant according to the serde enum representation.
fn render_variant(
    variant: &crate::types::Variant,
    representation: &EnumRepresentation,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    // Resolve `output_override` literally, as [`generate_struct_block`] does.
    let variant = variant.effective();
    let mut all_annotations: Vec<String> = Vec::new();
    all_annotations.extend(variant.annotations.iter().cloned());

    let ann_prefix = if !all_annotations.is_empty() {
        let joined = all_annotations
            .iter()
            .map(|a| format!("/** {} */", a))
            .collect::<Vec<_>>()
            .join(" ");
        format!("{} ", joined)
    } else {
        String::new()
    };

    let type_str = match representation {
        EnumRepresentation::ExternallyTagged => {
            render_variant_externally_tagged(variant, array_style, registry)
        }
        EnumRepresentation::InternallyTagged { tag } => {
            render_variant_internally_tagged(variant, tag, array_style, registry)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            render_variant_adjacently_tagged(variant, tag, content, array_style, registry)
        }
        EnumRepresentation::Untagged => render_variant_untagged(variant, array_style, registry),
    };

    format!("{}{}", ann_prefix, type_str)
}

/// ExternallyTagged: `{ VariantName: Type }` for data variants, `"VariantName"` for unit.
fn render_variant_externally_tagged(
    variant: &crate::types::Variant,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match &variant.data {
        Some(VariantData::InlineStruct(s)) => {
            format!(
                "{{ {}: {} }}",
                variant.name,
                inline_struct_type(s, array_style, registry)
            )
        }
        Some(VariantData::DataStructureRef(ft)) => {
            format!(
                "{{ {}: {} }}",
                variant.name,
                field_type_to_typescript(ft, array_style, registry)
            )
        }
        None => format!("\"{}\"", variant.name),
    }
}

/// InternallyTagged: all variants become objects with the tag field as a literal discriminator.
/// InlineStruct: `{ tag: 'VariantName' } & { ...fields }` intersection.
/// DataStructureRef (newtype variants): `{ tag: 'VariantName' } & TypeRef` intersection.
/// Unit variants: `{ tag: 'VariantName' }`.
fn render_variant_internally_tagged(
    variant: &crate::types::Variant,
    tag: &str,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match &variant.data {
        Some(VariantData::InlineStruct(s)) => format!(
            "{{ {}: '{}' }} & {}",
            tag,
            variant.name,
            inline_struct_type(s, array_style, registry)
        ),
        Some(VariantData::DataStructureRef(ft)) => {
            // Serde flattens newtype variants wrapping structs when internally tagged.
            // Use an intersection type: `{ tag: 'VariantName' } & TypeRef`
            format!(
                "{{ {}: '{}' }} & {}",
                tag,
                variant.name,
                field_type_to_typescript(ft, array_style, registry)
            )
        }
        None => format!("{{ {}: '{}' }}", tag, variant.name),
    }
}

/// AdjacentlyTagged: `{ tag: 'VariantName'; content: Type }` for data variants,
/// `{ tag: 'VariantName' }` for unit.
fn render_variant_adjacently_tagged(
    variant: &crate::types::Variant,
    tag: &str,
    content: &str,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match &variant.data {
        Some(VariantData::InlineStruct(s)) => {
            format!(
                "{{ {}: '{}'; {}: {} }}",
                tag,
                variant.name,
                content,
                inline_struct_type(s, array_style, registry)
            )
        }
        Some(VariantData::DataStructureRef(ft)) => {
            format!(
                "{{ {}: '{}'; {}: {} }}",
                tag,
                variant.name,
                content,
                field_type_to_typescript(ft, array_style, registry)
            )
        }
        None => format!("{{ {}: '{}' }}", tag, variant.name),
    }
}

/// Untagged: bare type reference, no wrapping.
fn render_variant_untagged(
    variant: &crate::types::Variant,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match &variant.data {
        Some(VariantData::InlineStruct(s)) => inline_struct_type(s, array_style, registry),
        Some(VariantData::DataStructureRef(ft)) => {
            field_type_to_typescript(ft, array_style, registry)
        }
        None => format!("\"{}\"", variant.name),
    }
}

/// A struct variant's fields as a TypeScript object type, as serde writes
/// them inline.
fn inline_struct_type(
    inline: &StructConfig,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let members = inline
        .fields
        .iter()
        .map(|field| {
            let field = field.effective();
            format!(
                "{}: {};",
                field.field_name.to_case(Case::Camel),
                field_type_to_typescript(&field.field_type, array_style, registry)
            )
        })
        .collect::<Vec<_>>();
    format!("{{ {} }}", members.join(" "))
}

/// Render a complete field block including annotations, @serde, and the field declaration.
/// This handles both inline @serde (for RecordLink fields) and separate-line @serde.
fn render_field_block(
    field: &crate::types::StructField,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    // Resolve `output_override` literally, as [`generate_struct_block`] does.
    let field = field.effective();
    let mut lines: Vec<String> = Vec::new();

    // 1. Field annotations
    for ann in &field.annotations {
        lines.push(format!("  /** {} */", ann));
    }

    // 2. Compute validators and serde annotation
    let validators_str = collect_validators_for_field(&field.validators, &field.field_type);
    let (serde_annotation, is_inline) =
        build_serde_annotation(&validators_str, &field.field_type, registry);

    // 3. Legacy doccom handling (for backwards compatibility)
    if let Some(ref dc) = field.doccom {
        // Split doccom by newline and render each part as a separate annotation
        for part in dc.split('\n') {
            let part = part.trim();
            if !part.is_empty() {
                lines.push(format!("  /** {} */", part.replace("*/", "* /")));
            }
        }
    }

    // 4. If not inline, render @serde as separate line(s) above the field
    if !is_inline && !serde_annotation.is_empty() {
        for serde_line in serde_annotation.split('\n') {
            lines.push(format!("  {}", serde_line));
        }
    }

    // 5. Field declaration line
    let field_name = field.field_name.to_case(Case::Camel);
    let type_str = if is_inline && !serde_annotation.is_empty() {
        render_field_type(
            &field.field_type,
            &serde_annotation,
            true,
            array_style,
            registry,
        )
    } else {
        field_type_to_typescript(&field.field_type, array_style, registry)
    };

    lines.push(format!("  {}: {};", field_name, type_str));

    lines.join("\n")
}

/// Format the `@derive(...)` JSDoc line from a list of macro names.
/// Falls back to `["Deserialize"]` when the vec is empty, preserving current behavior.
fn format_derive_line(derives: &[String]) -> String {
    if derives.is_empty() {
        "/** @derive(Deserialize) */".to_string()
    } else {
        format!("/** @derive({}) */", derives.join(", "))
    }
}

/// Convert a FieldType to its TypeScript representation.
fn field_type_to_typescript(
    field_type: &FieldType,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let render = |inner: &FieldType| field_type_to_typescript(inner, array_style, registry);
    match field_type {
        FieldType::String | FieldType::Char => "string".to_string(),
        FieldType::Bool => "boolean".to_string(),
        FieldType::Unit => "null".to_string(),
        FieldType::F32
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
        | FieldType::Usize => "number".to_string(),
        FieldType::Option(inner) => {
            format!("{} | null", wrap_union_type(inner, array_style, registry))
        }
        FieldType::Vec(inner) => format_array(inner, array_style, registry),
        FieldType::Tuple(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        FieldType::Struct(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(name, field)| format!("{name}: {}", render(field)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        FieldType::RecordLink(inner) => record_link_type(render(inner), registry),
        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            let map_key = MapKey::of(key);
            let key_type = match map_key {
                Ok(MapKey::Bool) => BOOL_KEYS
                    .iter()
                    .map(|key| format!("\"{key}\""))
                    .collect::<Vec<_>>()
                    .join(" | "),
                _ => render(key),
            };
            let record = format!("Record<{key_type}, {}>", render(value));
            match map_key {
                Ok(map_key) if map_key.is_finite(registry) => format!("Partial<{record}>"),
                _ => record,
            }
        }
        FieldType::Other(type_name) => match registry
            .lookup(type_name)
            .and_then(|foreign| foreign.macroforge.as_ref())
        {
            Some(mapping) => mapping.type_expr.clone(),
            None => type_name.to_case(Case::Pascal),
        },
    }
}

/// A record link to `linked`, as the project's `RecordLink` foreign type
/// writes it, or as evenframe's own `RecordLink`.
fn record_link_type(linked: String, registry: &crate::types::ForeignTypeRegistry) -> String {
    match registry
        .lookup(RECORD_LINK)
        .and_then(|record_link| record_link.macroforge.as_ref())
    {
        Some(mapping) => fill(&mapping.type_expr, &[linked]),
        None => format!("RecordLink<{linked}>"),
    }
}

/// Format a Vec type as either `Type[]` (shorthand) or `Array<Type>` (generic).
fn format_array(
    inner: &FieldType,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match array_style {
        ArrayStyle::Shorthand => {
            format!("{}[]", wrap_union_type(inner, array_style, registry))
        }
        ArrayStyle::Generic => {
            format!(
                "Array<{}>",
                field_type_to_typescript(inner, array_style, registry)
            )
        }
    }
}

/// Render a field type for use as an inner type in Option (and, for the
/// shorthand array style, for Vec as well).
/// Wraps Option in parentheses for correct `Type[]` semantics; not needed
/// for generic `Array<Type>` syntax since the angle brackets handle grouping.
fn wrap_union_type(
    ft: &FieldType,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let rendered = field_type_to_typescript(ft, array_style, registry);
    if matches!(ft, FieldType::Option(_)) && array_style == ArrayStyle::Shorthand {
        format!("({rendered})")
    } else {
        rendered
    }
}

/// Collect validators and format them as a comma-separated string for JSDoc.
/// For String and bare RecordLink fields, automatically adds "nonEmpty" unless already present;
/// a char field is held to exactly one character.
fn collect_validators_for_field(validators: &[Validator], field_type: &FieldType) -> String {
    let mut result: Vec<String> = validators
        .iter()
        .filter_map(validator_to_macroforge_string)
        .collect();

    // Add nonEmpty for String fields by default (RecordLink handles this in its own type definition)
    if matches!(field_type, FieldType::String) && !result.iter().any(|v| v == "nonEmpty") {
        result.insert(0, "nonEmpty".to_string());
    }
    if matches!(field_type, FieldType::Char) {
        result.insert(
            0,
            format!(
                "pattern({})",
                escape_for_jsdoc(crate::typesync::js_checks::ONE_CHARACTER)
            ),
        );
    }

    result
        .iter()
        .map(|v| format!("\"{}\"", v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Compute `@serde({ format: "..." })` annotation for field types that need it.
/// Returns None if no format annotation is needed.
fn collect_serde_format(
    field_type: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> Option<String> {
    if let FieldType::Other(name) = field_type
        && let Some(ftc) = registry.lookup(name)
        && !ftc.serde_format.is_empty()
    {
        return Some(format!(
            "/** @serde({{ format: \"{}\" }}) */",
            ftc.serde_format
        ));
    }
    None
}

/// Build the full serde annotation string for a field.
/// Combines validate and format annotations as needed.
/// Returns the annotation line (or empty string), and a boolean indicating
/// whether the serde should be rendered inline (for RecordLink fields).
fn build_serde_annotation(
    validators_str: &str,
    field_type: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> (String, bool) {
    let format_ann = collect_serde_format(field_type, registry);
    let is_record_link = matches!(field_type, FieldType::RecordLink(_));

    if !validators_str.is_empty()
        && let Some(format_line) = format_ann
    {
        // Both validate and format: render as separate lines (validate first)
        let validate_line = format!("/** @serde({{ validate: [{}] }}) */", validators_str);
        (
            format!("{}\n{}", validate_line, format_line),
            is_record_link,
        )
    } else if !validators_str.is_empty() {
        (
            format!("/** @serde({{ validate: [{}] }}) */", validators_str),
            is_record_link,
        )
    } else if let Some(fmt) = format_ann {
        (fmt, false)
    } else {
        (String::new(), false)
    }
}

/// Render the field type, with optional inline @serde for RecordLink fields.
fn render_field_type(
    field_type: &FieldType,
    serde_annotation: &str,
    inline: bool,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    if inline && !serde_annotation.is_empty() {
        // For RecordLink, render @serde inline: /** @serde(...) */ RecordLink<Type>
        if let FieldType::RecordLink(inner) = field_type {
            return format!(
                "{serde_annotation} {}",
                record_link_type(
                    field_type_to_typescript(inner, array_style, registry),
                    registry
                )
            );
        }
    }
    field_type_to_typescript(field_type, array_style, registry)
}

/// Derives macroforge provides itself, which need no `import macro`.
const BUILT_IN_DERIVES: [&str; 9] = [
    "Clone",
    "Debug",
    "Default",
    "Deserialize",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "Serialize",
];

/// The `import macro` lines for the derives the written types in `type_names` carry,
/// one per package, with each derive macroforge does not provide imported
/// from its package in `macros`. A derive with no package there is an error.
pub fn macro_import_lines(
    type_names: &[String],
    index: &TypeIndex,
    macros: &BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let type_set: BTreeSet<&str> = type_names.iter().map(String::as_str).collect();
    // Read through the views `generate_struct_block` and
    // `generate_enum_block` write, so the imports match each `@derive(...)`.
    let derives = index
        .named_structs()
        .iter()
        .filter(|(name, struct_config)| {
            !struct_config.resolve_only && type_set.contains(name.as_str())
        })
        .flat_map(|(_, struct_config)| &struct_view(struct_config).macroforge_derives)
        .chain(
            index
                .named_enums()
                .iter()
                .filter(|(name, tagged_union)| {
                    !tagged_union.resolve_only && type_set.contains(name.as_str())
                })
                .flat_map(|(_, tagged_union)| &enum_view(tagged_union).macroforge_derives),
        );
    let mut seen = BTreeSet::new();
    let mut by_package: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut unimportable = Vec::new();
    for derive in derives {
        if BUILT_IN_DERIVES.contains(&derive.as_str()) || !seen.insert(derive.as_str()) {
            continue;
        }
        match macros.get(derive) {
            Some(package) => by_package.entry(package).or_default().push(derive),
            None => unimportable.push(format!("`{derive}`")),
        }
    }
    if let Some(first) = unimportable.first() {
        return Err(EvenframeError::config(format!(
            "the macroforge output derives {}, which macroforge does not provide, and names no \
             package to import them from. Name each one's package in the output's `macros`, \
             such as macros = {{ {} = \"@scope/macros\" }}",
            unimportable.join(", "),
            first.trim_matches('`')
        )));
    }
    Ok(by_package
        .into_iter()
        .map(|(package, derives)| {
            format!(
                "/** import macro {{{}}} from \"{package}\"; */",
                derives.join(", ")
            )
        })
        .collect())
}

/// The imports a set of types needs for the foreign types they use, each from
/// where its macroforge mapping says, including a configured `RecordLink`.
///
/// Follows referenced structs and enums into other files too: macroforge's
/// expansion inlines variant payloads into the parent's generated code, so the
/// parent needs their foreign imports even where it never names them.
pub fn compute_extra_imports(
    type_names: &[String],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> ExtraImports {
    let used = foreign_types_used(
        type_names,
        index,
        registry,
        &Reading {
            struct_view,
            enum_view,
            expands_held_types: true,
        },
    );
    let configured_record_link = registry
        .lookup(RECORD_LINK)
        .and_then(|record_link| record_link.macroforge.as_ref());
    let mut imports: Vec<&crate::config::TsImport> = used
        .foreign
        .values()
        .filter_map(|foreign| foreign.macroforge.as_ref())
        .filter_map(|mapping| mapping.import.as_ref())
        .collect();
    if used.record_link {
        imports.extend(configured_record_link.and_then(|mapping| mapping.import.as_ref()));
    }
    ExtraImports {
        lines: import_lines(imports),
        needs_record_link: used.record_link && configured_record_link.is_none(),
    }
}

/// Convert a Validator to its Macroforge string representation.
/// Returns None for transformation validators that should be skipped.
fn validator_to_macroforge_string(validator: &Validator) -> Option<String> {
    match validator {
        Validator::StringValidator(sv) => string_validator_to_macroforge(sv),
        Validator::NumberValidator(nv) => number_validator_to_macroforge(nv),
        Validator::ArrayValidator(av) => array_validator_to_macroforge(av),
        Validator::DateValidator(dv) => date_validator_to_macroforge(dv),
        Validator::BigIntValidator(biv) => bigint_validator_to_macroforge(biv),
        Validator::BigDecimalValidator(bdv) => bigdecimal_validator_to_macroforge(bdv),
        Validator::DurationValidator(dv) => duration_validator_to_macroforge(dv),
    }
}

fn string_validator_to_macroforge(sv: &StringValidator) -> Option<String> {
    match sv {
        // Length validators
        StringValidator::MinLength(n) => Some(format!("minLength({})", n)),
        StringValidator::MaxLength(n) => Some(format!("maxLength({})", n)),
        StringValidator::Length(n) => Some(format!("length({})", n)),
        StringValidator::NonEmpty => Some("nonEmpty".to_string()),

        // Format validators
        StringValidator::Email => Some("email".to_string()),
        StringValidator::Url => Some("url".to_string()),
        StringValidator::Uuid
        | StringValidator::UuidV1
        | StringValidator::UuidV2
        | StringValidator::UuidV3
        | StringValidator::UuidV4
        | StringValidator::UuidV5
        | StringValidator::UuidV6
        | StringValidator::UuidV7
        | StringValidator::UuidV8 => Some("uuid".to_string()),
        StringValidator::Ip => Some("ip".to_string()),
        StringValidator::IpV4 => Some("ipv4".to_string()),
        StringValidator::IpV6 => Some("ipv6".to_string()),
        StringValidator::CreditCard => Some("creditCard".to_string()),
        StringValidator::Semver => Some("semver".to_string()),
        StringValidator::Json => Some("json".to_string()),
        StringValidator::Base64 => Some("base64".to_string()),
        StringValidator::Base64Url => Some("base64Url".to_string()),

        // Character type validators
        StringValidator::Alpha => Some("alpha".to_string()),
        StringValidator::Alphanumeric => Some("alphanumeric".to_string()),
        StringValidator::Digits => Some("digits".to_string()),
        StringValidator::Hex => Some("hex".to_string()),
        StringValidator::Integer => Some("integer".to_string()),
        StringValidator::Numeric => Some("numeric".to_string()),

        // Case/state validators (validation-only, not transformations)
        StringValidator::Lowercased | StringValidator::LowerPreformatted => {
            Some("lowercase".to_string())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted => {
            Some("uppercase".to_string())
        }
        StringValidator::Trimmed | StringValidator::TrimPreformatted => Some("trimmed".to_string()),
        StringValidator::Capitalized | StringValidator::CapitalizePreformatted => {
            Some("capitalized".to_string())
        }
        StringValidator::Uncapitalized => Some("uncapitalized".to_string()),

        // Substring validators
        StringValidator::StartsWith(s) => Some(format!("startsWith(\"{}\")", escape_for_jsdoc(s))),
        StringValidator::EndsWith(s) => Some(format!("endsWith(\"{}\")", escape_for_jsdoc(s))),
        StringValidator::Includes(s) => Some(format!("includes(\"{}\")", escape_for_jsdoc(s))),

        // Pattern validators
        StringValidator::RegexLiteral(format) => {
            Some(format!("pattern({})", escape_for_jsdoc(&format.pattern())))
        }
        StringValidator::Literal(s) => Some(format!("literal(\"{}\")", escape_for_jsdoc(s))),

        // Date validators
        StringValidator::Date => Some("date".to_string()),
        StringValidator::DateIso => Some("dateIso".to_string()),
        StringValidator::DateEpoch => Some("dateEpoch".to_string()),

        // Skip transformation validators - these modify data rather than validate
        StringValidator::String
        | StringValidator::Capitalize
        | StringValidator::Lower
        | StringValidator::Upper
        | StringValidator::Trim
        | StringValidator::Normalize
        | StringValidator::NormalizeNFC
        | StringValidator::NormalizeNFD
        | StringValidator::NormalizeNFKC
        | StringValidator::NormalizeNFKD
        | StringValidator::NormalizeNFCPreformatted
        | StringValidator::NormalizeNFDPreformatted
        | StringValidator::NormalizeNFKCPreformatted
        | StringValidator::NormalizeNFKDPreformatted
        | StringValidator::DateParse
        | StringValidator::DateEpochParse
        | StringValidator::DateIsoParse
        | StringValidator::IntegerParse
        | StringValidator::NumericParse
        | StringValidator::JsonParse
        | StringValidator::UrlParse
        | StringValidator::Regex
        | StringValidator::StringEmbedded(_) => None,
    }
}

fn number_validator_to_macroforge(nv: &NumberValidator) -> Option<String> {
    match nv {
        NumberValidator::Int => Some("int".to_string()),
        NumberValidator::Finite => Some("finite".to_string()),
        NumberValidator::NonNaN => Some("nonNaN".to_string()),
        NumberValidator::Positive => Some("positive".to_string()),
        NumberValidator::Negative => Some("negative".to_string()),
        NumberValidator::NonPositive => Some("nonPositive".to_string()),
        NumberValidator::NonNegative => Some("nonNegative".to_string()),
        NumberValidator::GreaterThan(n) => Some(format!("greaterThan({})", n.0)),
        NumberValidator::GreaterThanOrEqualTo(n) => Some(format!("greaterThanOrEqualTo({})", n.0)),
        NumberValidator::LessThan(n) => Some(format!("lessThan({})", n.0)),
        NumberValidator::LessThanOrEqualTo(n) => Some(format!("lessThanOrEqualTo({})", n.0)),
        NumberValidator::Between(start, end) => Some(format!("between({}, {})", start.0, end.0)),
        NumberValidator::MultipleOf(n) => Some(format!("multipleOf({})", n.0)),
        NumberValidator::Uint8 => Some("uint8".to_string()),
    }
}

fn array_validator_to_macroforge(av: &ArrayValidator) -> Option<String> {
    match av {
        ArrayValidator::MinItems(n) => Some(format!("minItems({})", n)),
        ArrayValidator::MaxItems(n) => Some(format!("maxItems({})", n)),
        ArrayValidator::ItemsCount(n) => Some(format!("itemsCount({})", n)),
    }
}

fn date_validator_to_macroforge(dv: &DateValidator) -> Option<String> {
    match dv {
        DateValidator::ValidDate => Some("validDate".to_string()),
        DateValidator::GreaterThanDate(d) => {
            Some(format!("greaterThanDate(\"{}\")", escape_for_jsdoc(d)))
        }
        DateValidator::GreaterThanOrEqualToDate(d) => Some(format!(
            "greaterThanOrEqualToDate(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DateValidator::LessThanDate(d) => {
            Some(format!("lessThanDate(\"{}\")", escape_for_jsdoc(d)))
        }
        DateValidator::LessThanOrEqualToDate(d) => Some(format!(
            "lessThanOrEqualToDate(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DateValidator::BetweenDate(start, end) => Some(format!(
            "betweenDate(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn bigint_validator_to_macroforge(biv: &BigIntValidator) -> Option<String> {
    match biv {
        BigIntValidator::PositiveBigInt => Some("positiveBigInt".to_string()),
        BigIntValidator::NegativeBigInt => Some("negativeBigInt".to_string()),
        BigIntValidator::NonPositiveBigInt => Some("nonPositiveBigInt".to_string()),
        BigIntValidator::NonNegativeBigInt => Some("nonNegativeBigInt".to_string()),
        BigIntValidator::GreaterThanBigInt(n) => {
            Some(format!("greaterThanBigInt(\"{}\")", escape_for_jsdoc(n)))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(n) => Some(format!(
            "greaterThanOrEqualToBigInt(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigIntValidator::LessThanBigInt(n) => {
            Some(format!("lessThanBigInt(\"{}\")", escape_for_jsdoc(n)))
        }
        BigIntValidator::LessThanOrEqualToBigInt(n) => Some(format!(
            "lessThanOrEqualToBigInt(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigIntValidator::BetweenBigInt(start, end) => Some(format!(
            "betweenBigInt(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn bigdecimal_validator_to_macroforge(bdv: &BigDecimalValidator) -> Option<String> {
    match bdv {
        BigDecimalValidator::PositiveBigDecimal => Some("positiveBigDecimal".to_string()),
        BigDecimalValidator::NegativeBigDecimal => Some("negativeBigDecimal".to_string()),
        BigDecimalValidator::NonPositiveBigDecimal => Some("nonPositiveBigDecimal".to_string()),
        BigDecimalValidator::NonNegativeBigDecimal => Some("nonNegativeBigDecimal".to_string()),
        BigDecimalValidator::GreaterThanBigDecimal(n) => Some(format!(
            "greaterThanBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(n) => Some(format!(
            "greaterThanOrEqualToBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::LessThanBigDecimal(n) => {
            Some(format!("lessThanBigDecimal(\"{}\")", escape_for_jsdoc(n)))
        }
        BigDecimalValidator::LessThanOrEqualToBigDecimal(n) => Some(format!(
            "lessThanOrEqualToBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::BetweenBigDecimal(start, end) => Some(format!(
            "betweenBigDecimal(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn duration_validator_to_macroforge(dv: &DurationValidator) -> Option<String> {
    match dv {
        DurationValidator::GreaterThanDuration(d) => {
            Some(format!("greaterThanDuration(\"{}\")", escape_for_jsdoc(d)))
        }
        DurationValidator::GreaterThanOrEqualToDuration(d) => Some(format!(
            "greaterThanOrEqualToDuration(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DurationValidator::LessThanDuration(d) => {
            Some(format!("lessThanDuration(\"{}\")", escape_for_jsdoc(d)))
        }
        DurationValidator::LessThanOrEqualToDuration(d) => Some(format!(
            "lessThanOrEqualToDuration(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DurationValidator::BetweenDuration(start, end) => Some(format!(
            "betweenDuration(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

/// Escape special characters for JSDoc strings.
fn escape_for_jsdoc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Extract derive names from a typesync override string.
/// Parses `/** @derive(Default, Serialize, Deserialize, Gigaform, Overview) */`
/// and returns `["Default", "Serialize", "Deserialize", "Gigaform", "Overview"]`.
#[cfg(test)]
mod tests {
    use super::{
        ArrayStyle, ArrayValidator, BTreeMap, FieldType, NumberValidator, StringValidator,
        StructConfig, TaggedUnion, TypeIndex, Validator, collect_validators_for_field,
        compute_extra_imports, field_type_to_typescript, generate_macroforge_for_types,
        generate_macroforge_type_string, macro_import_lines, validator_to_macroforge_string,
    };
    use crate::types::{EnumRepresentation, Pipeline, StructField, Variant};
    use ordered_float::OrderedFloat;

    #[test]
    fn test_string_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Email)),
            Some("email".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::MinLength(8)
            )),
            Some("minLength(8)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::MaxLength(50)
            )),
            Some("maxLength(50)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Uuid)),
            Some("uuid".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::Lowercased
            )),
            Some("lowercase".to_string())
        );
    }

    #[test]
    fn test_number_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Int)),
            Some("int".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(18.0),
                OrderedFloat(120.0)
            ))),
            Some("between(18, 120)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Positive)),
            Some("positive".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::ArrayValidator(ArrayValidator::MinItems(1))),
            Some("minItems(1)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::ArrayValidator(ArrayValidator::MaxItems(5))),
            Some("maxItems(5)".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_skipped() {
        // These should return None as they're transformations, not validations
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Lower)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Upper)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Trim)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::IntegerParse
            )),
            None
        );
    }

    #[test]
    fn test_field_type_to_typescript() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let s = ArrayStyle::Shorthand;
        assert_eq!(
            field_type_to_typescript(&FieldType::String, s, &registry),
            "string"
        );
        assert_eq!(
            field_type_to_typescript(&FieldType::Bool, s, &registry),
            "boolean"
        );
        assert_eq!(
            field_type_to_typescript(&FieldType::I32, s, &registry),
            "number"
        );
        assert_eq!(
            field_type_to_typescript(&FieldType::F64, s, &registry),
            "number"
        );
        assert!(
            field_type_to_typescript(
                &FieldType::Option(Box::new(FieldType::String)),
                s,
                &registry
            )
            .contains("string")
                && field_type_to_typescript(
                    &FieldType::Option(Box::new(FieldType::String)),
                    s,
                    &registry
                )
                .contains("null")
        );
        let vec_output =
            field_type_to_typescript(&FieldType::Vec(Box::new(FieldType::I32)), s, &registry);
        assert!(vec_output.contains("number") && vec_output.contains("[]"));
        assert!(
            field_type_to_typescript(&FieldType::Other("UserProfile".to_string()), s, &registry)
                .contains("UserProfile")
        );
    }

    #[test]
    fn test_field_type_to_typescript_generic_array_style() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let g = ArrayStyle::Generic;
        // Vec<i32> → Array<number>
        let vec_output =
            field_type_to_typescript(&FieldType::Vec(Box::new(FieldType::I32)), g, &registry);
        assert!(
            vec_output.contains("Array<number>"),
            "Expected Array<number>, got: {}",
            vec_output
        );
        assert!(
            !vec_output.contains("[]"),
            "Generic style should not contain [], got: {}",
            vec_output
        );
        // Vec<Option<String>> → Array<string | null>
        let vec_opt = field_type_to_typescript(
            &FieldType::Vec(Box::new(FieldType::Option(Box::new(FieldType::String)))),
            g,
            &registry,
        );
        assert!(
            vec_opt.contains("Array<") && vec_opt.contains("string") && vec_opt.contains("null"),
            "Expected Array<string | null>, got: {}",
            vec_opt
        );
    }

    #[test]
    fn test_field_type_to_typescript_exact_output() {
        let registry = make_datetime_registry();
        let render = |field_type: FieldType, style: ArrayStyle| {
            field_type_to_typescript(&field_type, style, &registry)
        };
        let s = ArrayStyle::Shorthand;
        let g = ArrayStyle::Generic;
        assert_eq!(render(FieldType::Unit, s), "null");
        assert_eq!(render(FieldType::Char, s), "string");
        assert_eq!(render(FieldType::U64, s), "number");
        assert_eq!(
            render(FieldType::Other("user_profile".to_string()), s),
            "UserProfile"
        );
        assert_eq!(
            render(FieldType::Other("DateTime".to_string()), s),
            "DateTime.Utc"
        );
        assert_eq!(
            render(FieldType::Option(Box::new(FieldType::String)), s),
            "string | null"
        );
        assert_eq!(
            render(
                FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::I32)))),
                s
            ),
            "number[] | null"
        );
        assert_eq!(
            render(
                FieldType::Vec(Box::new(FieldType::Option(Box::new(FieldType::String)))),
                s
            ),
            "(string | null)[]"
        );
        assert_eq!(
            render(
                FieldType::Vec(Box::new(FieldType::Option(Box::new(FieldType::String)))),
                g
            ),
            "Array<string | null>"
        );
        assert_eq!(
            render(
                FieldType::Tuple(vec![
                    FieldType::String,
                    FieldType::Option(Box::new(FieldType::I32))
                ]),
                s
            ),
            "[string, number | null]"
        );
        assert_eq!(
            render(
                FieldType::Struct(vec![
                    ("a".to_string(), FieldType::String),
                    ("b".to_string(), FieldType::Vec(Box::new(FieldType::Bool))),
                ]),
                s
            ),
            "{ a: string; b: boolean[] }"
        );
        assert_eq!(
            render(
                FieldType::HashMap(Box::new(FieldType::String), Box::new(FieldType::I64)),
                s
            ),
            "Record<string, number>"
        );
        assert_eq!(
            render(
                FieldType::BTreeMap(
                    Box::new(FieldType::String),
                    Box::new(FieldType::Other("DateTime".to_string()))
                ),
                s
            ),
            "Record<string, DateTime.Utc>"
        );
        assert_eq!(
            render(
                FieldType::RecordLink(Box::new(FieldType::Other("user".to_string()))),
                s
            ),
            "RecordLink<User>"
        );
    }

    #[test]
    fn test_collect_validators_for_string_adds_nonempty() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        // String fields get nonEmpty added by default
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\", \"minLength(5)\""
        );
    }

    #[test]
    fn test_collect_validators_for_number_no_nonempty() {
        let validators = vec![Validator::NumberValidator(NumberValidator::Int)];
        // Number fields don't get nonEmpty
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::I32),
            "\"int\""
        );
    }

    #[test]
    fn test_collect_validators_skips_transformations() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::Lower), // Should be skipped
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        // nonEmpty is added first for strings
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\", \"minLength(5)\""
        );
    }

    #[test]
    fn test_collect_validators_doesnt_duplicate_nonempty() {
        let validators = vec![
            Validator::StringValidator(StringValidator::NonEmpty),
            Validator::StringValidator(StringValidator::Email),
        ];
        // NonEmpty already present, shouldn't be duplicated
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\""
        );
    }

    #[test]
    fn test_generate_complete_interface() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user_registration_form".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![Validator::StringValidator(StringValidator::Email)],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "password".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::MinLength(8)),
                            Validator::StringValidator(StringValidator::MaxLength(50)),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::I32,
                        validators: vec![
                            Validator::NumberValidator(NumberValidator::Int),
                            Validator::NumberValidator(NumberValidator::Between(
                                OrderedFloat(18.0),
                                OrderedFloat(120.0),
                            )),
                        ],
                        ..Default::default()
                    },
                ],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            ArrayStyle::default(),
            &registry,
        );

        assert!(output.contains("/** @derive(Deserialize) */"));
        assert!(output.contains("export interface UserRegistrationForm"));
        // String fields now get nonEmpty by default
        assert!(output.contains("@serde({ validate: [\"nonEmpty\", \"email\"] })"));
        assert!(
            output.contains(
                "@serde({ validate: [\"nonEmpty\", \"minLength(8)\", \"maxLength(50)\"] })"
            )
        );
        // Number fields don't get nonEmpty
        assert!(output.contains("@serde({ validate: [\"int\", \"between(18, 120)\"] })"));
        assert!(output.contains("email: string"));
        assert!(output.contains("password: string"));
        assert!(output.contains("age: number"));
    }

    #[test]
    fn test_custom_macroforge_derives_and_annotations() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "account".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "account".to_string(),
                fields: vec![
                    StructField {
                        field_name: "title".to_string(),
                        field_type: FieldType::String,
                        annotations: vec![
                            "@textController({ label: \"Title\" })".to_string(),
                            "@overviewColumn({ heading: \"Title\" })".to_string(),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "status".to_string(),
                        field_type: FieldType::String,
                        ..Default::default()
                    },
                ],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![
                    "Default".to_string(),
                    "Serialize".to_string(),
                    "Deserialize".to_string(),
                    "Gigaform".to_string(),
                    "Overview".to_string(),
                ],
                annotations: vec![
                    "@overview({ dataName: \"account\", apiUrl: \"/api/accounts\" })".to_string(),
                ],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let mut enums = BTreeMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                resolve_only: false,
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Scheduled".to_string(),
                        data: None,
                        doccom: None,
                        annotations: vec!["@default".to_string()],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                    },
                    Variant {
                        name: "OnDeck".to_string(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                    },
                ],
                doccom: None,
                macroforge_derives: vec![
                    "Default".to_string(),
                    "Serialize".to_string(),
                    "Deserialize".to_string(),
                ],
                annotations: vec![],
                representation: EnumRepresentation::default(),
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &TypeIndex::new(&structs, &enums).unwrap(),
            ArrayStyle::default(),
            &registry,
        );

        // Struct: custom derives
        assert!(
            output.contains("/** @derive(Default, Serialize, Deserialize, Gigaform, Overview) */"),
            "Should contain custom derives. Output:\n{}",
            output
        );
        // Struct: type-level annotation
        assert!(
            output
                .contains("/** @overview({ dataName: \"account\", apiUrl: \"/api/accounts\" }) */"),
            "Should contain struct-level annotation. Output:\n{}",
            output
        );
        // Struct: field-level annotations
        assert!(
            output.contains("/** @textController({ label: \"Title\" }) */"),
            "Should contain field-level textController annotation. Output:\n{}",
            output
        );
        assert!(
            output.contains("/** @overviewColumn({ heading: \"Title\" }) */"),
            "Should contain field-level overviewColumn annotation. Output:\n{}",
            output
        );

        // Enum: custom derives
        assert!(
            output.contains("/** @derive(Default, Serialize, Deserialize) */"),
            "Should contain enum custom derives. Output:\n{}",
            output
        );
        // Enum: variant-level annotation
        assert!(
            output.contains("@default"),
            "Should contain variant-level @default annotation. Output:\n{}",
            output
        );
    }

    #[test]
    fn test_empty_macroforge_derives_falls_back_to_deserialize() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "simple".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "simple".to_string(),
                fields: vec![],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            ArrayStyle::default(),
            &registry,
        );
        assert!(
            output.contains("/** @derive(Deserialize) */"),
            "Empty macroforge_derives should fall back to Deserialize. Output:\n{}",
            output
        );
    }

    fn make_datetime_registry() -> crate::types::ForeignTypeRegistry {
        use crate::config::ForeignTypeConfig;
        let mut foreign_types = BTreeMap::new();
        // DateTime and BigDecimal are type-only re-exports from the
        // `effect` package; the registry is configured for `import type`
        // so generated TS doesn't pull runtime artifacts it doesn't need.
        foreign_types.insert(
            "DateTime".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["DateTime".to_string()],
                macroforge: Some(crate::config::TsMapping {
                    type_expr: "DateTime.Utc".to_string(),
                    import: Some(crate::config::TsImport {
                        from: "effect".to_string(),
                        name: "DateTime".to_string(),
                        type_only: true,
                    }),
                }),
                ..Default::default()
            },
        );
        foreign_types.insert(
            "Decimal".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["Decimal".to_string()],
                macroforge: Some(crate::config::TsMapping {
                    type_expr: "BigDecimal.BigDecimal".to_string(),
                    import: Some(crate::config::TsImport {
                        from: "effect".to_string(),
                        name: "BigDecimal".to_string(),
                        type_only: true,
                    }),
                }),
                ..Default::default()
            },
        );
        crate::types::ForeignTypeRegistry::from_config(&foreign_types)
    }

    #[test]
    fn test_datetime_maps_to_datetime_utc() {
        let registry = make_datetime_registry();
        let s = ArrayStyle::Shorthand;
        assert!(
            field_type_to_typescript(&FieldType::Other("DateTime".to_string()), s, &registry)
                .contains("DateTime.Utc")
        );
        // Option<DateTime> should produce DateTime.Utc | null
        let opt_dt = field_type_to_typescript(
            &FieldType::Option(Box::new(FieldType::Other("DateTime".to_string()))),
            s,
            &registry,
        );
        assert!(opt_dt.contains("DateTime.Utc") && opt_dt.contains("null"));
        // Vec<DateTime> should produce DateTime.Utc[]
        let vec_dt = field_type_to_typescript(
            &FieldType::Vec(Box::new(FieldType::Other("DateTime".to_string()))),
            s,
            &registry,
        );
        assert!(vec_dt.contains("DateTime.Utc") && vec_dt.contains("[]"));
    }

    #[test]
    fn test_decimal_maps_to_bigdecimal() {
        let registry = make_datetime_registry();
        let s = ArrayStyle::Shorthand;
        assert!(
            field_type_to_typescript(&FieldType::Other("Decimal".to_string()), s, &registry)
                .contains("BigDecimal.BigDecimal")
        );
        // Option<Decimal> should produce BigDecimal.BigDecimal | null
        let opt_dec = field_type_to_typescript(
            &FieldType::Option(Box::new(FieldType::Other("Decimal".to_string()))),
            s,
            &registry,
        );
        assert!(opt_dec.contains("BigDecimal.BigDecimal") && opt_dec.contains("null"));
    }

    #[test]
    fn test_compute_extra_imports_datetime() {
        let registry = make_datetime_registry();
        let mut structs = BTreeMap::new();
        structs.insert(
            "event".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "event".to_string(),
                fields: vec![StructField {
                    field_name: "starts_at".to_string(),
                    field_type: FieldType::Other("DateTime".to_string()),
                    ..Default::default()
                }],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let imports = compute_extra_imports(
            &["Event".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &registry,
        );
        assert_eq!(
            imports.lines,
            vec!["import type { DateTime } from 'effect';".to_string()]
        );
    }

    #[test]
    fn test_compute_extra_imports_decimal() {
        let registry = make_datetime_registry();
        let mut structs = BTreeMap::new();
        structs.insert(
            "payment".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "payment".to_string(),
                fields: vec![StructField {
                    field_name: "amount".to_string(),
                    field_type: FieldType::Other("Decimal".to_string()),
                    ..Default::default()
                }],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let imports = compute_extra_imports(
            &["Payment".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &registry,
        );
        assert_eq!(
            imports.lines,
            vec!["import type { BigDecimal } from 'effect';".to_string()]
        );
    }

    #[test]
    fn derives_are_imported_from_their_configured_packages() {
        let derived = |struct_name: &str, derives: &[&str]| StructConfig {
            struct_name: struct_name.to_string(),
            macroforge_derives: derives.iter().map(|derive| derive.to_string()).collect(),
            ..Default::default()
        };
        let structs = BTreeMap::from([
            (
                "Order".to_string(),
                derived("Order", &["Debug", "Form", "Serialize"]),
            ),
            (
                "Invoice".to_string(),
                derived("Invoice", &["Overview", "Form", "Audit"]),
            ),
        ]);
        let types = ["Order".to_string(), "Invoice".to_string()];
        let macros = BTreeMap::from([
            ("Form".to_string(), "@app/forms".to_string()),
            ("Overview".to_string(), "@app/forms".to_string()),
            ("Audit".to_string(), "@app/audit".to_string()),
        ]);
        assert_eq!(
            macro_import_lines(
                &types,
                &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
                &macros
            )
            .unwrap(),
            vec![
                "/** import macro {Audit} from \"@app/audit\"; */".to_string(),
                "/** import macro {Overview, Form} from \"@app/forms\"; */".to_string(),
            ]
        );
        let unconfigured = macro_import_lines(
            &types,
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &BTreeMap::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            unconfigured.contains("`Overview`, `Form`, `Audit`") && !unconfigured.contains("Debug"),
            "{unconfigured}"
        );
    }

    #[test]
    fn imports_follow_embedded_types_but_not_tables() {
        let registry = make_datetime_registry();
        let field = |field_name: &str, field_type: FieldType| StructField {
            field_name: field_name.to_string(),
            field_type,
            ..Default::default()
        };
        let dated = |struct_name: &str| StructConfig {
            struct_name: struct_name.to_string(),
            fields: vec![field("at", FieldType::Other("DateTime".to_string()))],
            ..Default::default()
        };
        let holder = |struct_name: &str, fields: Vec<StructField>| StructConfig {
            struct_name: struct_name.to_string(),
            fields,
            ..Default::default()
        };
        let structs = BTreeMap::from([
            ("event".to_string(), dated("Event")),
            ("Slot".to_string(), dated("Slot")),
            (
                "Booking".to_string(),
                holder(
                    "Booking",
                    vec![field("slot", FieldType::Other("Slot".to_string()))],
                ),
            ),
            (
                "Invite".to_string(),
                holder(
                    "Invite",
                    vec![
                        field(
                            "event",
                            FieldType::RecordLink(Box::new(FieldType::Other("Event".to_string()))),
                        ),
                        field("host", FieldType::Other("Event".to_string())),
                    ],
                ),
            ),
        ]);
        let imports_of = |type_name: &str| {
            compute_extra_imports(
                &[type_name.to_string()],
                &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
                &registry,
            )
            .lines
        };
        assert_eq!(
            imports_of("Booking"),
            vec!["import type { DateTime } from 'effect';".to_string()]
        );
        assert!(imports_of("Invite").is_empty());
    }

    #[test]
    fn a_configured_record_link_is_imported_and_not_declared() {
        use crate::config::{ForeignTypeConfig, TsImport, TsMapping};
        let registry = crate::types::ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "RecordLink".to_string(),
            ForeignTypeConfig {
                macroforge: Some(TsMapping {
                    type_expr: "RecordLink<{0}>".to_string(),
                    import: Some(TsImport {
                        from: "./index".to_string(),
                        name: "RecordLink".to_string(),
                        type_only: true,
                    }),
                }),
                ..Default::default()
            },
        )]));
        let order = StructConfig {
            struct_name: "Order".to_string(),
            fields: vec![StructField {
                field_name: "customer".to_string(),
                field_type: FieldType::RecordLink(Box::new(FieldType::Other(
                    "Customer".to_string(),
                ))),
                ..Default::default()
            }],
            ..Default::default()
        };
        let structs = BTreeMap::from([("Order".to_string(), order)]);
        let imports = compute_extra_imports(
            &["Order".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &registry,
        );
        assert_eq!(
            imports.lines,
            vec!["import type { RecordLink } from './index';".to_string()]
        );
        assert!(!imports.needs_record_link);
        let without_entry = compute_extra_imports(
            &["Order".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &crate::types::ForeignTypeRegistry::default(),
        );
        assert!(without_entry.needs_record_link);
    }

    #[test]
    fn test_compute_extra_imports_both() {
        let registry = make_datetime_registry();
        let mut structs = BTreeMap::new();
        structs.insert(
            "order".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "order".to_string(),
                fields: vec![
                    StructField {
                        field_name: "amount".to_string(),
                        field_type: FieldType::Other("Decimal".to_string()),
                        ..Default::default()
                    },
                    StructField {
                        field_name: "created_at".to_string(),
                        field_type: FieldType::Other("DateTime".to_string()),
                        ..Default::default()
                    },
                ],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let imports = compute_extra_imports(
            &["Order".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &registry,
        );
        assert_eq!(
            imports.lines,
            vec![
                "import type { BigDecimal } from 'effect';".to_string(),
                "import type { DateTime } from 'effect';".to_string(),
            ]
        );
    }

    #[test]
    fn test_compute_extra_imports_none_needed() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut structs = BTreeMap::new();
        structs.insert(
            "user".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user".to_string(),
                fields: vec![StructField {
                    field_name: "name".to_string(),
                    field_type: FieldType::String,
                    ..Default::default()
                }],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let imports = compute_extra_imports(
            &["User".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &registry,
        );
        assert!(imports.lines.is_empty());
        assert!(!imports.needs_record_link);
    }

    #[test]
    fn test_generate_macroforge_for_types_with_annotations() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "order".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "order".to_string(),
                fields: vec![StructField {
                    field_name: "amount".to_string(),
                    field_type: FieldType::F64,
                    annotations: vec!["@currency({ symbol: \"$\" })".to_string()],
                    ..Default::default()
                }],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec!["Serialize".to_string(), "Deserialize".to_string()],
                annotations: vec!["@overview({ dataName: \"order\" })".to_string()],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_for_types(
            &["Order".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            ArrayStyle::default(),
            &registry,
        );

        assert!(
            output.contains("/** @derive(Serialize, Deserialize) */"),
            "Should contain custom derives in per-file mode. Output:\n{}",
            output
        );
        assert!(
            output.contains("/** @overview({ dataName: \"order\" }) */"),
            "Should contain type-level annotation in per-file mode. Output:\n{}",
            output
        );
        assert!(
            output.contains("/** @currency({ symbol: \"$\" }) */"),
            "Should contain field-level annotation in per-file mode. Output:\n{}",
            output
        );
    }
}
