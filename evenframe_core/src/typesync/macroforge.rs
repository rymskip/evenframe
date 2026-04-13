//! Macroforge TypeScript interface generation with JSDoc validator annotations.
//!
//! This module generates TypeScript interfaces with `@derive(Deserialize)` at the type level
//! and `@serde({ validate: [...] })` annotations at the field level for validators.

use crate::types::{EnumRepresentation, FieldType, StructConfig, TaggedUnion, VariantData};
use crate::typesync::config::ArrayStyle;
use crate::typesync::doc_comment::format_jsdoc;
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use macroforge_ts::macros::ts_template;
use std::collections::{HashMap, HashSet};

/// Main entry point for generating Macroforge TypeScript interfaces.
pub fn generate_macroforge_type_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    _print_types: bool,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating Macroforge TypeScript interfaces"
    );

    // Deduplicate structs by PascalCase name to avoid generating the same interface twice
    let mut seen_structs = HashSet::new();
    let mut unique_structs: Vec<&StructConfig> = structs
        .values()
        .filter(|s| {
            let name = s.struct_name.to_case(Case::Pascal);
            if seen_structs.contains(&name) {
                false
            } else {
                seen_structs.insert(name);
                true
            }
        })
        .collect();
    unique_structs.sort_by_key(|s| s.struct_name.to_case(Case::Pascal));

    // Deduplicate enums by PascalCase name
    let mut seen_enums = HashSet::new();
    let mut unique_enums: Vec<&TaggedUnion> = enums
        .values()
        .filter(|e| {
            let name = e.enum_name.to_case(Case::Pascal);
            if seen_enums.contains(&name) {
                false
            } else {
                seen_enums.insert(name);
                true
            }
        })
        .collect();
    unique_enums.sort_by_key(|e| e.enum_name.to_case(Case::Pascal));

    // Collect all type names for effect import computation
    let all_type_names: Vec<String> = unique_structs
        .iter()
        .map(|s| s.struct_name.to_case(Case::Pascal))
        .chain(
            unique_enums
                .iter()
                .map(|e| e.enum_name.to_case(Case::Pascal)),
        )
        .collect();

    let mut result = String::new();
    let extra_imports = compute_extra_imports(&all_type_names, structs, enums, registry);
    if !extra_imports.is_empty() {
        result.push_str(&extra_imports.join("\n"));
        result.push_str("\n\n");
    }

    let mut parts: Vec<String> = Vec::new();
    for struct_config in &unique_structs {
        parts.push(generate_struct_block(struct_config, array_style, registry));
    }
    for enum_def in &unique_enums {
        parts.push(generate_enum_block(enum_def, array_style, registry));
    }

    result.push_str(&parts.join("\n"));
    tracing::info!(
        output_length = result.len(),
        "Macroforge interface generation complete"
    );
    result
}

/// Generates Macroforge TypeScript interfaces for a specific subset of types (used in per-file mode).
pub fn generate_macroforge_for_types(
    type_names: &[String],
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let type_set: HashSet<String> = type_names.iter().cloned().collect();

    // Filter to only the requested types, deduplicating by PascalCase name.
    let mut seen_structs = HashSet::new();
    let mut filtered_structs: Vec<&StructConfig> = structs
        .values()
        .filter(|s| {
            let name = s.struct_name.to_case(Case::Pascal);
            if !type_set.contains(&name) || seen_structs.contains(&name) {
                false
            } else {
                seen_structs.insert(name);
                true
            }
        })
        .collect();
    filtered_structs.sort_by_key(|s| s.struct_name.to_case(Case::Pascal));

    let mut seen_enums = HashSet::new();
    let mut filtered_enums: Vec<&TaggedUnion> = enums
        .values()
        .filter(|e| {
            let name = e.enum_name.to_case(Case::Pascal);
            if !type_set.contains(&name) || seen_enums.contains(&name) {
                false
            } else {
                seen_enums.insert(name);
                true
            }
        })
        .collect();
    filtered_enums.sort_by_key(|e| e.enum_name.to_case(Case::Pascal));

    let mut parts: Vec<String> = Vec::new();
    for struct_config in &filtered_structs {
        parts.push(generate_struct_block(struct_config, array_style, registry));
    }
    for enum_def in &filtered_enums {
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
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let mut lines: Vec<String> = Vec::new();

    // Use the override config if provided, otherwise use the original
    let effective = struct_config
        .output_override
        .as_deref()
        .unwrap_or(struct_config);
    let derive_line = format_derive_line(&effective.macroforge_derives);
    if let Some(ref desc) = effective.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.push(derive_line);
    for ann in &effective.annotations {
        lines.push(format!("/** {} */", ann));
    }
    lines.push(format!("export interface {} {{", name));
    for field in &struct_config.fields {
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
    let name = enum_def.enum_name.to_case(Case::Pascal);
    let mut lines: Vec<String> = Vec::new();

    let effective = enum_def.output_override.as_deref().unwrap_or(enum_def);
    let derive_line = format_derive_line(&effective.macroforge_derives);
    if let Some(ref desc) = effective.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.push(derive_line);
    for ann in &effective.annotations {
        lines.push(format!("/** {} */", ann));
    }

    // Emit @serde annotation for tagged representations so the macroforge
    // type registry knows how to parse/stringify these unions at runtime.
    match &enum_def.representation {
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

    let variant_parts: Vec<String> = enum_def
        .variants
        .iter()
        .map(|variant| render_variant(variant, &enum_def.representation, array_style, registry))
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
                s.struct_name.to_case(Case::Pascal)
            )
        }
        Some(VariantData::DataStructureRef(ft)) => {
            format!(
                "{{ {}: {} }}",
                variant.name,
                field_type_to_typescript(ft, array_style, registry).trim()
            )
        }
        None => format!("\"{}\"", variant.name),
    }
}

/// InternallyTagged: all variants become objects with the tag field as a literal discriminator.
/// InlineStruct: `{ tag: 'VariantName' } & StructName` intersection.
/// DataStructureRef (newtype variants): `{ tag: 'VariantName' } & TypeRef` intersection.
/// Unit variants: `{ tag: 'VariantName' }`.
fn render_variant_internally_tagged(
    variant: &crate::types::Variant,
    tag: &str,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match &variant.data {
        Some(VariantData::InlineStruct(s)) => {
            // Use intersection: { tag: 'VariantName' } & StructName
            format!(
                "{{ {}: '{}' }} & {}",
                tag,
                variant.name,
                s.struct_name.to_case(Case::Pascal)
            )
        }
        Some(VariantData::DataStructureRef(ft)) => {
            // Serde flattens newtype variants wrapping structs when internally tagged.
            // Use an intersection type: `{ tag: 'VariantName' } & TypeRef`
            format!(
                "{{ {}: '{}' }} & {}",
                tag,
                variant.name,
                field_type_to_typescript(ft, array_style, registry).trim()
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
                s.struct_name.to_case(Case::Pascal)
            )
        }
        Some(VariantData::DataStructureRef(ft)) => {
            format!(
                "{{ {}: '{}'; {}: {} }}",
                tag,
                variant.name,
                content,
                field_type_to_typescript(ft, array_style, registry).trim()
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
        Some(VariantData::InlineStruct(s)) => s.struct_name.to_case(Case::Pascal),
        Some(VariantData::DataStructureRef(ft)) => {
            field_type_to_typescript(ft, array_style, registry)
                .trim()
                .to_string()
        }
        None => format!("\"{}\"", variant.name),
    }
}

/// Render a complete field block including annotations, @serde, and the field declaration.
/// This handles both inline @serde (for RecordLink fields) and separate-line @serde.
fn render_field_block(
    field: &crate::types::StructField,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    // 1. Field annotations — use override if present, otherwise original
    let effective = field.output_override.as_deref().unwrap_or(field);
    for ann in &effective.annotations {
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

    lines.push(format!("  {}: {};", field_name, type_str.trim()));

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
    // Check for foreign type in Other variant before using ts_template
    if let FieldType::Other(type_name) = field_type
        && let Some(ftc) = registry.lookup(type_name)
        && !ftc.macroforge.is_empty()
    {
        return format!(" {}", ftc.macroforge);
    }
    ts_template! {
        {#match field_type}
            {:case FieldType::String | FieldType::Char}
                string
            {:case FieldType::Bool}
                boolean
            {:case FieldType::Unit}
                null
            {:case FieldType::F32 | FieldType::F64}
                number
            {:case FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 | FieldType::Isize}
                number
            {:case FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 | FieldType::Usize}
                number
            {:case FieldType::Option(inner)}
                @{wrap_union_type(inner, array_style, registry)} | null
            {:case FieldType::Vec(inner)}
                @{format_array(inner, array_style, registry)}
            {:case FieldType::Tuple(items)}
                [@{items.iter().map(|ft| field_type_to_typescript(ft, array_style, registry)).collect::<Vec<_>>().join(", ")}]
            {:case FieldType::Struct(fields)}
                { @{fields.iter().map(|(name, ft)| format!("{}: {}", name, field_type_to_typescript(ft, array_style, registry))).collect::<Vec<_>>().join("; ")} }
            {:case FieldType::RecordLink(inner)}
                RecordLink<@{field_type_to_typescript(inner, array_style, registry).trim()}>
            {:case FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value)}
                { [key: @{field_type_to_typescript(key, array_style, registry)}]: @{field_type_to_typescript(value, array_style, registry)} }
            {:case FieldType::Other(type_name)}
                @{type_name.to_case(Case::Pascal)}
        {/match}
    }
    .source()
    .to_string()
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
                field_type_to_typescript(inner, array_style, registry).trim()
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
    let trimmed = rendered.trim();
    if matches!(ft, FieldType::Option(_)) && array_style == ArrayStyle::Shorthand {
        format!("({})", trimmed)
    } else {
        trimmed.to_string()
    }
}

/// Collect validators and format them as a comma-separated string for JSDoc.
/// For String and bare RecordLink fields, automatically adds "nonEmpty" unless already present.
fn collect_validators_for_field(validators: &[Validator], field_type: &FieldType) -> String {
    let mut result: Vec<String> = validators
        .iter()
        .filter_map(validator_to_macroforge_string)
        .collect();

    // Add nonEmpty for String/Char fields by default (RecordLink handles this in its own type definition)
    if matches!(field_type, FieldType::String | FieldType::Char)
        && !result.iter().any(|v| v == "nonEmpty")
    {
        result.insert(0, "nonEmpty".to_string());
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
        // Both validate and format — render as separate lines (validate first)
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
                "{} RecordLink<{}>",
                serde_annotation,
                field_type_to_typescript(inner, array_style, registry).trim()
            );
        }
    }
    field_type_to_typescript(field_type, array_style, registry)
}

/// Compute the `/** import macro {...} from "@dealdraft/macros"; */` line
/// from the derives of all types in a file. Collects non-standard derives
/// (excluding Default, Serialize, Deserialize) and deduplicates them.
pub fn compute_macro_import_line(
    type_names: &[String],
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> Option<String> {
    let type_set: HashSet<String> = type_names.iter().cloned().collect();
    let standard_derives: HashSet<&str> = ["Default", "Serialize", "Deserialize"]
        .iter()
        .copied()
        .collect();

    let mut extra_derives: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    for s in structs.values() {
        let name = s.struct_name.to_case(Case::Pascal);
        if type_set.contains(&name) {
            let effective = s.output_override.as_deref().unwrap_or(s);
            let derives = effective.macroforge_derives.clone();
            for d in &derives {
                if !standard_derives.contains(d.as_str()) && seen.insert(d.clone()) {
                    extra_derives.push(d.clone());
                }
            }
        }
    }

    for e in enums.values() {
        let name = e.enum_name.to_case(Case::Pascal);
        if type_set.contains(&name) {
            let effective = e.output_override.as_deref().unwrap_or(e);
            let derives = effective.macroforge_derives.clone();
            for d in &derives {
                if !standard_derives.contains(d.as_str()) && seen.insert(d.clone()) {
                    extra_derives.push(d.clone());
                }
            }
        }
    }

    if extra_derives.is_empty() {
        None
    } else {
        Some(format!(
            "/** import macro {{{}}} from \"@dealdraft/macros\"; */",
            extra_derives.join(", ")
        ))
    }
}

/// Checks whether a FieldType tree contains a specific variant (recursively).
fn field_type_contains(ft: &FieldType, predicate: &dyn Fn(&FieldType) -> bool) -> bool {
    if predicate(ft) {
        return true;
    }
    match ft {
        FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::RecordLink(inner) => {
            field_type_contains(inner, predicate)
        }
        FieldType::HashMap(k, v) | FieldType::BTreeMap(k, v) => {
            field_type_contains(k, predicate) || field_type_contains(v, predicate)
        }
        FieldType::Tuple(items) => items
            .iter()
            .any(|item| field_type_contains(item, predicate)),
        FieldType::Struct(fields) => fields
            .iter()
            .any(|(_, ft)| field_type_contains(ft, predicate)),
        _ => false,
    }
}

/// Computes extra import lines needed for a set of types.
///
/// Emits:
/// - one import line per foreign type referenced by the given types whose
///   TS import is declared in the passed `registry` (built from the user's
///   `[general.foreign_types]` config — no foreign types are hardcoded);
/// - a single `RecordLink` utility import if any field in the types uses it.
pub fn compute_extra_imports(
    type_names: &[String],
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Vec<String> {
    let type_set: HashSet<String> = type_names.iter().cloned().collect();
    let mut needs_record_link = false;
    let mut foreign_imports: HashMap<String, bool> = HashMap::new();

    fn collect_foreign_imports_recursive(
        ft: &FieldType,
        registry: &crate::types::ForeignTypeRegistry,
        fi: &mut HashMap<String, bool>,
    ) {
        if let FieldType::Other(name) = ft
            && let Some(ftc) = registry.lookup(name)
            && !ftc.ts_import.is_empty()
        {
            fi.insert(ftc.ts_import.name.clone(), ftc.ts_import.is_type_only);
        }
        match ft {
            FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::RecordLink(inner) => {
                collect_foreign_imports_recursive(inner, registry, fi)
            }
            FieldType::HashMap(k, v) | FieldType::BTreeMap(k, v) => {
                collect_foreign_imports_recursive(k, registry, fi);
                collect_foreign_imports_recursive(v, registry, fi);
            }
            FieldType::Tuple(items) => {
                for item in items {
                    collect_foreign_imports_recursive(item, registry, fi);
                }
            }
            FieldType::Struct(fields) => {
                for (_, inner_ft) in fields {
                    collect_foreign_imports_recursive(inner_ft, registry, fi);
                }
            }
            _ => {}
        }
    }

    let check_field_type = |ft: &FieldType, rl: &mut bool, fi: &mut HashMap<String, bool>| {
        if field_type_contains(ft, &|f| matches!(f, FieldType::RecordLink(_))) {
            *rl = true;
        }
        collect_foreign_imports_recursive(ft, registry, fi);
    };

    for s in structs.values() {
        if type_set.contains(&s.struct_name.to_case(Case::Pascal)) {
            for field in &s.fields {
                check_field_type(
                    &field.field_type,
                    &mut needs_record_link,
                    &mut foreign_imports,
                );
            }
        }
    }

    for e in enums.values() {
        if type_set.contains(&e.enum_name.to_case(Case::Pascal)) {
            for variant in &e.variants {
                if let Some(data) = &variant.data {
                    match data {
                        VariantData::InlineStruct(_) => {
                            // Inline structs render as intersections, so their
                            // field-level imports are transitive (belong to the
                            // struct's own file, not this enum's file).
                        }
                        VariantData::DataStructureRef(ft) => {
                            check_field_type(ft, &mut needs_record_link, &mut foreign_imports);
                        }
                    }
                }
            }
        }
    }

    let mut lines: Vec<String> = Vec::new();

    // Foreign type imports from effect library
    if !foreign_imports.is_empty() {
        let mut sorted_imports: Vec<(String, bool)> = foreign_imports.into_iter().collect();
        sorted_imports.sort_by(|a, b| a.0.cmp(&b.0));
        for (import_name, is_type_only) in sorted_imports {
            let keyword = if is_type_only {
                "import type"
            } else {
                "import"
            };
            lines.push(format!("{} {{ {} }} from 'effect';", keyword, import_name));
        }
    }

    // RecordLink from local index
    if needs_record_link {
        lines.push("import type { RecordLink } from './index';".to_string());
    }

    lines
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
            let regex = format.clone().into_regex();
            Some(format!("pattern({})", escape_for_jsdoc(regex.as_str())))
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
    use super::*;
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
        // ts_template! adds whitespace, so we trim for comparison
        assert!(field_type_to_typescript(&FieldType::String, s, &registry).trim() == "string");
        assert!(field_type_to_typescript(&FieldType::Bool, s, &registry).trim() == "boolean");
        assert!(field_type_to_typescript(&FieldType::I32, s, &registry).trim() == "number");
        assert!(field_type_to_typescript(&FieldType::F64, s, &registry).trim() == "number");
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
            vec_output.trim()
        );
        assert!(
            !vec_output.contains("[]"),
            "Generic style should not contain [], got: {}",
            vec_output.trim()
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
            vec_opt.trim()
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
        let mut structs = HashMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &structs,
            &HashMap::new(),
            true,
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
    fn test_comment_syntax_comparison() {
        // This test demonstrates the difference between raw comments and interpolated comments
        // in ts_template!

        // Raw comment syntax - gets converted to #[doc = "..."] by the macro
        let raw_comment_output = ts_template! {
            /** @derive(Deserialize) */
            export interface Test {}
        }
        .source()
        .to_string();

        // Interpolated comment syntax - preserved as literal string
        let interpolated_comment_output = ts_template! {
            @{"/** @derive(Deserialize) */"}
            export interface Test {}
        }
        .source()
        .to_string();

        println!("Raw comment output: {:?}", raw_comment_output);
        println!(
            "Interpolated comment output: {:?}",
            interpolated_comment_output
        );

        // Raw comments are converted to Rust doc syntax (with spaces: "# [doc = ...")
        assert!(
            raw_comment_output.contains("doc") && raw_comment_output.contains("@derive"),
            "Raw comments should be converted to doc attribute syntax"
        );

        // Interpolated comments preserve the JSDoc format
        assert!(
            interpolated_comment_output.contains("/** @derive(Deserialize) */"),
            "Interpolated comments should preserve JSDoc syntax"
        );
    }

    #[test]
    fn test_custom_macroforge_derives_and_annotations() {
        let mut structs = HashMap::new();
        structs.insert(
            "account".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let mut enums = HashMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Scheduled".to_string(),
                        data: None,
                        doccom: None,
                        annotations: vec!["@default".to_string()],
                        output_override: None,
                        raw_attributes: std::collections::HashMap::new(),
                    },
                    Variant {
                        name: "OnDeck".to_string(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::HashMap::new(),
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &structs,
            &enums,
            true,
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
        let mut structs = HashMap::new();
        structs.insert(
            "simple".to_string(),
            StructConfig {
                struct_name: "simple".to_string(),
                fields: vec![],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_type_string(
            &structs,
            &HashMap::new(),
            true,
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
        let mut foreign_types = HashMap::new();
        // DateTime and BigDecimal are type-only re-exports from the
        // `effect` package; the registry is configured for `import type`
        // so generated TS doesn't pull runtime artifacts it doesn't need.
        foreign_types.insert(
            "DateTime".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["DateTime".to_string()],
                macroforge: "DateTime.Utc".to_string(),
                ts_import: crate::config::TsImport {
                    name: "DateTime".to_string(),
                    is_type_only: true,
                },
                ..Default::default()
            },
        );
        foreign_types.insert(
            "Decimal".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["Decimal".to_string()],
                macroforge: "BigDecimal.BigDecimal".to_string(),
                ts_import: crate::config::TsImport {
                    name: "BigDecimal".to_string(),
                    is_type_only: true,
                },
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
        let mut structs = HashMap::new();
        structs.insert(
            "event".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let imports =
            compute_extra_imports(&["Event".to_string()], &structs, &HashMap::new(), &registry);
        assert_eq!(
            imports,
            vec!["import type { DateTime } from 'effect';".to_string()]
        );
    }

    #[test]
    fn test_compute_extra_imports_decimal() {
        let registry = make_datetime_registry();
        let mut structs = HashMap::new();
        structs.insert(
            "payment".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let imports = compute_extra_imports(
            &["Payment".to_string()],
            &structs,
            &HashMap::new(),
            &registry,
        );
        assert_eq!(
            imports,
            vec!["import type { BigDecimal } from 'effect';".to_string()]
        );
    }

    #[test]
    fn test_compute_extra_imports_both() {
        let registry = make_datetime_registry();
        let mut structs = HashMap::new();
        structs.insert(
            "order".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let imports =
            compute_extra_imports(&["Order".to_string()], &structs, &HashMap::new(), &registry);
        assert_eq!(
            imports,
            vec![
                "import type { BigDecimal } from 'effect';".to_string(),
                "import type { DateTime } from 'effect';".to_string(),
            ]
        );
    }

    #[test]
    fn test_compute_extra_imports_none_needed() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut structs = HashMap::new();
        structs.insert(
            "user".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let imports =
            compute_extra_imports(&["User".to_string()], &structs, &HashMap::new(), &registry);
        assert!(imports.is_empty());
    }

    #[test]
    fn test_generate_macroforge_for_types_with_annotations() {
        let mut structs = HashMap::new();
        structs.insert(
            "order".to_string(),
            StructConfig {
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
                raw_attributes: std::collections::HashMap::new(),
            },
        );

        let registry = crate::types::ForeignTypeRegistry::default();
        let output = generate_macroforge_for_types(
            &["Order".to_string()],
            &structs,
            &HashMap::new(),
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
