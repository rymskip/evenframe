//! Macroforge TypeScript interface generation with JSDoc validator annotations.
//!
//! This module generates TypeScript interfaces with `@derive(Decode)` at the type level
//! and `@endec({ validate: [...] })` annotations at the field level for validators.

use crate::config::{RECORD_LINK, TsMapping, fill};
use crate::error::{EvenframeError, Result};
use crate::schemasync::format::Format;
use crate::types::{
    EnumRepresentation, FieldType, NewtypeConfig, NewtypeKind, StructConfig, TaggedUnion,
    VariantData,
};
use crate::typesync::config::{ArrayStyle, OutputKind};
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{
    Reading, RecordLinkMapping, foreign_types_used, import_lines, record_link_mapping,
};
use crate::typesync::js_checks::{self, JsCheck, object_key, string_literal};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::typesync::type_index::TypeIndex;
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

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

/// What rendering a type needs: the array style, the foreign types, and the
/// scanned types, which say what a newtype is written as.
#[derive(Clone, Copy)]
struct Rendering<'a> {
    array_style: ArrayStyle,
    registry: &'a crate::types::ForeignTypeRegistry,
    index: &'a TypeIndex<'a>,
    /// The output's `default_derives`, for a type with none of its own.
    default_derives: Option<&'a [String]>,
}

/// Main entry point for generating Macroforge TypeScript interfaces.
pub fn generate_macroforge_type_string(
    index: &TypeIndex,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut HelperModule,
    default_derives: Option<&[String]>,
) -> Result<String> {
    let rendering = Rendering {
        array_style,
        registry,
        index,
        default_derives,
    };
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
    let unique_newtypes: Vec<(&String, &NewtypeConfig)> = index
        .named_newtypes()
        .filter(|(_, newtype)| !newtype.resolve_only)
        .collect();

    let all_type_names: Vec<String> = unique_structs
        .iter()
        .map(|(name, _)| name.clone())
        .chain(unique_enums.iter().map(|(name, _)| name.clone()))
        .chain(unique_newtypes.iter().map(|(name, _)| (*name).clone()))
        .collect();

    let mut result = String::new();
    let extra_imports = compute_extra_imports(&all_type_names, index, registry, true)?;
    if !extra_imports.lines.is_empty() {
        result.push_str(&extra_imports.lines.join("\n"));
        result.push_str("\n\n");
    }
    if let Some(own_record_link) = &extra_imports.own_record_link {
        result.push_str(&own_record_link.declaration());
        result.push_str("\n\n");
    }

    let mut parts: Vec<String> = Vec::new();
    for (_, struct_config) in &unique_structs {
        parts.push(generate_struct_block(struct_config, rendering, helpers)?);
    }
    for (_, enum_def) in &unique_enums {
        parts.push(generate_enum_block(enum_def, rendering, helpers)?);
    }
    for (name, newtype) in &unique_newtypes {
        parts.push(generate_newtype_block(name, newtype, rendering, helpers)?);
    }

    result.push_str(&parts.join("\n"));
    tracing::info!(
        output_length = result.len(),
        "Macroforge interface generation complete"
    );
    Ok(result)
}

/// Imports a set of types needs beyond each other, and the `RecordLink`
/// evenframe declares for them when they use one the project does not
/// configure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraImports<'a> {
    pub lines: Vec<String>,
    pub own_record_link: Option<OwnRecordLink<'a>>,
}

/// Evenframe's own record link: the id as the project's `RecordId` mapping
/// writes it, or the linked record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnRecordLink<'a> {
    pub record_id: &'a TsMapping,
}

impl OwnRecordLink<'_> {
    pub fn declaration(&self) -> String {
        format!(
            "export type RecordLink<T> = {} | T;",
            self.record_id.type_expr
        )
    }

    /// The declaration as a module of its own, after the import it needs.
    pub fn module(&self) -> String {
        let mut module: String = import_lines(self.record_id.import.as_ref())
            .into_iter()
            .map(|line| format!("{line}\n"))
            .collect();
        if !module.is_empty() {
            module.push('\n');
        }
        module.push_str(&self.declaration());
        module.push('\n');
        module
    }
}

/// Generates Macroforge TypeScript interfaces for a specific subset of types (used in per-file mode).
pub fn generate_macroforge_for_types(
    type_names: &[String],
    index: &TypeIndex,
    array_style: ArrayStyle,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut HelperModule,
    default_derives: Option<&[String]>,
) -> Result<String> {
    let rendering = Rendering {
        array_style,
        registry,
        index,
        default_derives,
    };
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
        parts.push(generate_struct_block(struct_config, rendering, helpers)?);
    }
    for (_, enum_def) in &filtered_enums {
        parts.push(generate_enum_block(enum_def, rendering, helpers)?);
    }
    for (name, newtype) in index
        .named_newtypes()
        .filter(|(name, _)| type_set.contains(name.as_str()))
    {
        parts.push(generate_newtype_block(name, newtype, rendering, helpers)?);
    }
    Ok(parts.join("\n"))
}

/// A newtype's declaration: its validators on `$Newtype` over the type serde
/// writes it as, which Macroforge brands. A tuple or unit struct is just that
/// type.
fn generate_newtype_block(
    name: &str,
    newtype: &NewtypeConfig,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    let view = newtype.effective();
    let mut lines: Vec<String> = Vec::new();
    if let Some(ref doc) = view.doccom {
        lines.push(format_jsdoc(doc, ""));
    }
    lines.extend(format_derive_line(rendered_derives(
        &view.macroforge_derives,
        rendering.default_derives,
        &[],
    )));
    for annotation in &view.annotations {
        lines.push(format!("/** {annotation} */"));
    }
    let (_, inner) = element_types(
        &newtype.inner,
        &newtype.element_validators,
        name,
        rendering,
        helpers,
    )?;
    let declared = match newtype.kind {
        NewtypeKind::Branded => {
            let validators =
                collect_validators_for_field(&newtype.validators, &newtype.inner, name, helpers)?;
            let (endec, _) =
                build_endec_annotation(&validators, &newtype.inner, rendering.registry);
            lines.extend(endec.lines().map(str::to_owned));
            format!("$Newtype<{inner}>")
        }
        NewtypeKind::Alias => inner,
    };
    lines.push(format!("export type {name} = {declared};"));
    lines.push(String::new());
    Ok(lines.join("\n"))
}

/// Generate a single struct's TypeScript interface block.
fn generate_struct_block(
    struct_config: &StructConfig,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    // Always emit the interface under the entry's own struct_name; pull
    // body content (fields, derives, annotations) from `struct_view` so a
    // same-name override (rule plugin) is honored while a redirect
    // override (synthetic projection) leaves the partial's own body intact.
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let view = struct_view(struct_config);
    let mut lines: Vec<String> = Vec::new();

    let derive_line = format_derive_line(rendered_derives(
        &view.macroforge_derives,
        rendering.default_derives,
        &DECODE,
    ));
    if let Some(ref desc) = view.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.extend(derive_line);
    for ann in &view.annotations {
        lines.push(format!("/** {} */", ann));
    }
    let mut members = Vec::new();
    let mut held_types = Vec::new();
    let mut index_types = Vec::new();
    let mut intersections = Vec::new();
    for field in view.fields.iter().map(crate::types::StructField::effective) {
        if field.wire.serde_flatten {
            // serde writes what the field holds beside its siblings.
            match field.field_type.flattened_map_value() {
                Some(value) => index_types.push(field_type_to_typescript(value, rendering)),
                None => intersections.push(field_type_to_typescript(&field.field_type, rendering)),
            }
            continue;
        }
        members.push(render_field_block(field, rendering, helpers)?);
        held_types.push(field_type_to_typescript(&field.field_type, rendering));
    }
    if !index_types.is_empty() {
        // A key of the map may share an object with every named field.
        let values: Vec<String> = index_types.into_iter().chain(held_types).collect();
        members.push(format!("  [key: string]: {};", values.join(" | ")));
    }
    if intersections.is_empty() {
        lines.push(format!("export interface {} {{", name));
        lines.extend(members);
        lines.push("}".to_string());
    } else {
        lines.push(format!("export type {} = {{", name));
        lines.extend(members);
        lines.push(format!("}} & {};", intersections.join(" & ")));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

/// Generate a single enum's TypeScript type block.
fn generate_enum_block(
    enum_def: &TaggedUnion,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    // Same approach as [`generate_struct_block`]: emit under the entry's own
    // enum_name, pull body from `enum_view` so same-name overrides are
    // applied while redirect overrides leave the entry's own body intact.
    let name = enum_def.enum_name.to_case(Case::Pascal);
    let view = enum_view(enum_def);
    let mut lines: Vec<String> = Vec::new();

    let derive_line = format_derive_line(rendered_derives(
        &view.macroforge_derives,
        rendering.default_derives,
        &DECODE,
    ));
    if let Some(ref desc) = view.doccom {
        lines.push(format_jsdoc(desc, ""));
    }
    lines.extend(derive_line);
    for ann in &view.annotations {
        lines.push(format!("/** {} */", ann));
    }

    // Emit @endec annotation for tagged representations so the macroforge
    // type registry knows how to parse/stringify these unions at runtime. An
    // enum with an untagged variant is read structurally: each tagged
    // variant's type spells out its tag, and comes first, as serde reads it.
    let partly_untagged = view
        .variants
        .iter()
        .any(|variant| variant.effective().wire.serde_untagged);
    let representation = if partly_untagged {
        &EnumRepresentation::Untagged
    } else {
        &view.representation
    };
    match representation {
        EnumRepresentation::InternallyTagged { tag } => {
            lines.push(format!(
                "/** @endec({{ tag: \"{}\" }}) */",
                escape_for_jsdoc(tag)
            ));
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            lines.push(format!(
                "/** @endec({{ tag: \"{}\", content: \"{}\" }}) */",
                escape_for_jsdoc(tag),
                escape_for_jsdoc(content)
            ));
        }
        EnumRepresentation::ExternallyTagged => {
            lines.push("/** @endec({ externallyTagged: true }) */".to_string());
        }
        EnumRepresentation::Untagged => {
            lines.push("/** @endec({ untagged: true }) */".to_string());
        }
    }

    let variant_parts: Vec<String> = view
        .variants
        .iter()
        .map(|variant| {
            render_variant(
                variant.effective(),
                variant
                    .effective()
                    .serde_representation(&view.representation),
                rendering,
                helpers,
            )
        })
        .collect::<Result<_>>()?;

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
    Ok(lines.join("\n"))
}

/// Render a single enum variant according to the serde enum representation.
fn render_variant(
    variant: &crate::types::Variant,
    representation: &EnumRepresentation,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
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
            render_variant_externally_tagged(variant, rendering, helpers)?
        }
        EnumRepresentation::InternallyTagged { tag } => {
            render_variant_internally_tagged(variant, tag, rendering, helpers)?
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            render_variant_adjacently_tagged(variant, tag, content, rendering, helpers)?
        }
        EnumRepresentation::Untagged => render_variant_untagged(variant, rendering, helpers)?,
    };

    Ok(format!("{}{}", ann_prefix, type_str))
}

/// ExternallyTagged: `{ VariantName: Type }` for data variants, `"VariantName"` for unit.
fn render_variant_externally_tagged(
    variant: &crate::types::Variant,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    let key = object_key(variant.serde_name())?;
    Ok(match &variant.data {
        Some(VariantData::InlineStruct(inline)) => format!(
            "{{ {key}: {} }}",
            inline_struct_type(inline, rendering, helpers)?
        ),
        Some(VariantData::DataStructureRef(field_type)) => {
            let (annotation, payload) = payload_type(variant, field_type, rendering, helpers)?;
            format!("{{ {annotation}{key}: {payload} }}")
        }
        None => string_literal(variant.serde_name())?,
    })
}

/// InternallyTagged: all variants become objects with the tag field as a literal discriminator.
/// InlineStruct: `{ tag: 'VariantName' } & { ...fields }` intersection.
/// DataStructureRef (newtype variants): `{ tag: 'VariantName' } & TypeRef` intersection.
/// Unit variants: `{ tag: 'VariantName' }`.
fn render_variant_internally_tagged(
    variant: &crate::types::Variant,
    tag: &str,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    let tag_entry = format!(
        "{{ {}: {} }}",
        object_key(tag)?,
        string_literal(variant.serde_name())?
    );
    Ok(match &variant.data {
        Some(VariantData::InlineStruct(inline)) => format!(
            "{tag_entry} & {}",
            inline_struct_type(inline, rendering, helpers)?
        ),
        // serde writes the tag into the struct a newtype variant holds.
        Some(VariantData::DataStructureRef(field_type)) => format!(
            "{tag_entry} & {}",
            field_type_to_typescript(field_type, rendering)
        ),
        None => tag_entry,
    })
}

/// AdjacentlyTagged: `{ tag: 'VariantName'; content: Type }` for data variants,
/// `{ tag: 'VariantName' }` for unit.
fn render_variant_adjacently_tagged(
    variant: &crate::types::Variant,
    tag: &str,
    content: &str,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    let tag_entry = format!(
        "{}: {}",
        object_key(tag)?,
        string_literal(variant.serde_name())?
    );
    let content = object_key(content)?;
    Ok(match &variant.data {
        Some(VariantData::InlineStruct(inline)) => format!(
            "{{ {tag_entry}; {content}: {} }}",
            inline_struct_type(inline, rendering, helpers)?
        ),
        Some(VariantData::DataStructureRef(field_type)) => {
            let (annotation, payload) = payload_type(variant, field_type, rendering, helpers)?;
            format!("{{ {tag_entry}; {annotation}{content}: {payload} }}")
        }
        None => format!("{{ {tag_entry} }}"),
    })
}

/// Untagged: bare type reference, no wrapping, and `null` for a unit variant,
/// which serde writes as null.
fn render_variant_untagged(
    variant: &crate::types::Variant,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    Ok(match &variant.data {
        Some(VariantData::InlineStruct(inline)) => inline_struct_type(inline, rendering, helpers)?,
        Some(VariantData::DataStructureRef(field_type)) => {
            let (annotation, payload) = payload_type(variant, field_type, rendering, helpers)?;
            format!("{annotation}{payload}")
        }
        None => "null".to_owned(),
    })
}

/// A tuple variant's payload type with each element's validators as `@endec`
/// before it, and for a newtype payload, its one element's annotation to put
/// before what holds it.
fn payload_type(
    variant: &crate::types::Variant,
    field_type: &FieldType,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<(String, String)> {
    element_types(
        field_type,
        &variant.element_validators,
        variant.serde_name(),
        rendering,
        helpers,
    )
}

/// A payload's TypeScript with its elements' validators, as [`payload_type`]
/// writes it.
fn element_types(
    field_type: &FieldType,
    element_validators: &[Vec<Validator>],
    owner: &str,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<(String, String)> {
    let mut annotation = |held: &FieldType, validators: &[Validator], name: &str| {
        let validators = collect_validators_for_field(validators, held, name, helpers)?;
        Ok::<_, EvenframeError>(if validators.is_empty() {
            String::new()
        } else {
            format!("/** @endec({{ validate: [{validators}] }}) */ ")
        })
    };
    match (field_type, element_validators) {
        (_, []) => Ok((
            String::new(),
            field_type_to_typescript(field_type, rendering),
        )),
        (FieldType::Tuple(items), validators) if items.len() == validators.len() => {
            let elements = items
                .iter()
                .zip(validators)
                .enumerate()
                .map(|(position, (item, validators))| {
                    Ok(format!(
                        "{}{}",
                        annotation(item, validators, &format!("{owner}.{position}"))?,
                        field_type_to_typescript(item, rendering)
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok((String::new(), format!("[{}]", elements.join(", "))))
        }
        (single, [validators]) => Ok((
            annotation(single, validators, owner)?,
            field_type_to_typescript(single, rendering),
        )),
        (_, validators) => Err(EvenframeError::config(format!(
            "`{owner}` has validators for {} elements but holds {field_type:?}",
            validators.len()
        ))),
    }
}

/// A struct variant's fields as a TypeScript object type, as serde writes
/// them inline, each with its validators.
fn inline_struct_type(
    inline: &StructConfig,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    let members = inline
        .fields
        .iter()
        .map(|field| {
            let field = field.effective();
            let validators = collect_validators_for_field(
                &field.validators,
                &field.field_type,
                &field.field_name,
                helpers,
            )?;
            let annotation = if validators.is_empty() {
                String::new()
            } else {
                format!("/** @endec({{ validate: [{validators}] }}) */ ")
            };
            Ok(format!(
                "{annotation}{}: {};",
                field_key(field)?,
                field_type_to_typescript(&field.field_type, rendering)
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{{ {} }}", members.join(" ")))
}

/// A field's key as serde writes it, optional when serde may leave it out.
fn field_key(field: &crate::types::StructField) -> Result<String> {
    let optional = if field.wire.serde_optional { "?" } else { "" };
    Ok(format!("{}{optional}", object_key(&field.ts_name())?))
}

/// Render a complete field block including annotations, @endec, and the field declaration.
/// This handles both inline @endec (for RecordLink fields) and separate-line @endec.
fn render_field_block(
    field: &crate::types::StructField,
    rendering: Rendering<'_>,
    helpers: &mut HelperModule,
) -> Result<String> {
    // Resolve `output_override` literally, as [`generate_struct_block`] does.
    let field = field.effective();
    let mut lines: Vec<String> = Vec::new();

    // 1. Field annotations
    for ann in &field.annotations {
        lines.push(format!("  /** {} */", ann));
    }

    // 2. Compute validators and endec annotation
    let validators_str = collect_validators_for_field(
        &field.validators,
        &field.field_type,
        &field.field_name,
        helpers,
    )?;
    let (endec_annotation, is_inline) =
        build_endec_annotation(&validators_str, &field.field_type, rendering.registry);

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

    // 4. If not inline, render @endec as separate line(s) above the field
    if !is_inline && !endec_annotation.is_empty() {
        for endec_line in endec_annotation.split('\n') {
            lines.push(format!("  {}", endec_line));
        }
    }

    // 5. Field declaration line
    let type_str = if is_inline && !endec_annotation.is_empty() {
        render_field_type(&field.field_type, &endec_annotation, true, rendering)
    } else {
        field_type_to_typescript(&field.field_type, rendering)
    };

    lines.push(format!("  {}: {};", field_key(field)?, type_str));

    Ok(lines.join("\n"))
}

/// Format the `@derive(...)` JSDoc line from a list of macro names.
/// Falls back to `["Decode"]` when no derives are configured.
/// What a struct or enum with no derives of its own and no `default_derives`
/// renders.
static DECODE: LazyLock<Vec<String>> = LazyLock::new(|| vec!["Decode".to_owned()]);

/// The derives a type renders: its own, else the output's `default_derives`,
/// else `fallback`.
fn rendered_derives<'a>(
    declared: &'a [String],
    default_derives: Option<&'a [String]>,
    fallback: &'a [String],
) -> &'a [String] {
    if declared.is_empty() {
        default_derives.unwrap_or(fallback)
    } else {
        declared
    }
}

fn format_derive_line(derives: &[String]) -> Option<String> {
    (!derives.is_empty()).then(|| format!("/** @derive({}) */", derives.join(", ")))
}

/// Convert a FieldType to its TypeScript representation.
fn field_type_to_typescript(field_type: &FieldType, rendering: Rendering<'_>) -> String {
    let render = |inner: &FieldType| field_type_to_typescript(inner, rendering);
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
        FieldType::Duration => render(&FieldType::serde_duration()),
        FieldType::Option(inner) => {
            format!("{} | null", wrap_union_type(inner, rendering))
        }
        FieldType::Vec(inner) => format_array(inner, rendering),
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
        FieldType::RecordLink(inner) => record_link_type(render(inner), rendering.registry),
        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            // A newtype key is written as the key it holds, unbranded.
            let key = rendering.index.underlying(key);
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
                Ok(map_key) if map_key.is_finite(rendering.registry) => {
                    format!("Partial<{record}>")
                }
                _ => record,
            }
        }
        FieldType::Other(type_name) => match rendering
            .registry
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
fn format_array(inner: &FieldType, rendering: Rendering<'_>) -> String {
    match rendering.array_style {
        ArrayStyle::Shorthand => {
            format!("{}[]", wrap_union_type(inner, rendering))
        }
        ArrayStyle::Generic => {
            format!("Array<{}>", field_type_to_typescript(inner, rendering))
        }
    }
}

/// Render a field type for use as an inner type in Option (and, for the
/// shorthand array style, for Vec as well).
/// Wraps Option in parentheses for correct `Type[]` semantics; not needed
/// for generic `Array<Type>` syntax since the angle brackets handle grouping.
fn wrap_union_type(ft: &FieldType, rendering: Rendering<'_>) -> String {
    let rendered = field_type_to_typescript(ft, rendering);
    if matches!(ft, FieldType::Option(_)) && rendering.array_style == ArrayStyle::Shorthand {
        format!("({rendered})")
    } else {
        rendered
    }
}

/// Compute `@endec({ format: "..." })` annotation for field types that need it.
/// Returns None if no format annotation is needed.
fn collect_endec_format(
    field_type: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> Option<String> {
    if let FieldType::Other(name) = field_type
        && let Some(ftc) = registry.lookup(name)
        && !ftc.endec_format.is_empty()
    {
        return Some(format!(
            "/** @endec({{ format: \"{}\" }}) */",
            ftc.endec_format
        ));
    }
    None
}

/// Build the full endec annotation string for a field.
/// Combines validate and format annotations as needed.
/// Returns the annotation line (or empty string), and a boolean indicating
/// whether the endec should be rendered inline (for RecordLink fields).
fn build_endec_annotation(
    validators_str: &str,
    field_type: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> (String, bool) {
    let format_ann = collect_endec_format(field_type, registry);
    let is_record_link = matches!(field_type, FieldType::RecordLink(_));

    if !validators_str.is_empty()
        && let Some(format_line) = format_ann
    {
        // Both validate and format: render as separate lines (validate first)
        let validate_line = format!("/** @endec({{ validate: [{}] }}) */", validators_str);
        (
            format!("{}\n{}", validate_line, format_line),
            is_record_link,
        )
    } else if !validators_str.is_empty() {
        (
            format!("/** @endec({{ validate: [{}] }}) */", validators_str),
            is_record_link,
        )
    } else if let Some(fmt) = format_ann {
        (fmt, false)
    } else {
        (String::new(), false)
    }
}

/// Render the field type, with optional inline @endec for RecordLink fields.
fn render_field_type(
    field_type: &FieldType,
    endec_annotation: &str,
    inline: bool,
    rendering: Rendering<'_>,
) -> String {
    if inline && !endec_annotation.is_empty() {
        // For RecordLink, render @endec inline: /** @endec(...) */ RecordLink<Type>
        if let FieldType::RecordLink(inner) = field_type {
            return format!(
                "{endec_annotation} {}",
                record_link_type(
                    field_type_to_typescript(inner, rendering),
                    rendering.registry
                )
            );
        }
    }
    field_type_to_typescript(field_type, rendering)
}

/// Derives macroforge provides itself, which need no `import macro`.
const BUILT_IN_DERIVES: [&str; 9] = [
    "Clone",
    "Debug",
    "Default",
    "Decode",
    "Encode",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
];

/// The `import macro` lines for the derives the written types in `type_names` carry,
/// one per package, with each derive macroforge does not provide imported
/// from its package in `macros`. A derive with no package there is an error.
pub fn macro_import_lines(
    type_names: &[String],
    index: &TypeIndex,
    macros: &BTreeMap<String, String>,
    default_derives: Option<&[String]>,
) -> Result<Vec<String>> {
    let type_set: BTreeSet<&str> = type_names.iter().map(String::as_str).collect();
    // Resolved as `generate_struct_block`, `generate_enum_block` and
    // `generate_newtype_block` resolve them, so the imports match each
    // `@derive(...)`.
    let derives = index
        .named_structs()
        .iter()
        .filter(|(name, struct_config)| {
            !struct_config.resolve_only && type_set.contains(name.as_str())
        })
        .flat_map(|(_, struct_config)| {
            rendered_derives(
                &struct_view(struct_config).macroforge_derives,
                default_derives,
                &DECODE,
            )
        })
        .chain(
            index
                .named_enums()
                .iter()
                .filter(|(name, tagged_union)| {
                    !tagged_union.resolve_only && type_set.contains(name.as_str())
                })
                .flat_map(|(_, tagged_union)| {
                    rendered_derives(
                        &enum_view(tagged_union).macroforge_derives,
                        default_derives,
                        &DECODE,
                    )
                }),
        )
        .chain(
            index
                .named_newtypes()
                .filter(|(name, newtype)| !newtype.resolve_only && type_set.contains(name.as_str()))
                .flat_map(|(_, newtype)| {
                    rendered_derives(
                        &newtype.effective().macroforge_derives,
                        default_derives,
                        &[],
                    )
                }),
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
/// where its macroforge mapping says, including a configured `RecordLink`,
/// and the `RecordId` evenframe's own `RecordLink` names when the same file
/// `declares_record_link`.
///
/// Follows referenced structs and enums into other files too: macroforge's
/// expansion inlines variant payloads into the parent's generated code, so the
/// parent needs their foreign imports even where it never names them.
pub fn compute_extra_imports<'a>(
    type_names: &[String],
    index: &TypeIndex,
    registry: &'a crate::types::ForeignTypeRegistry,
    declares_record_link: bool,
) -> Result<ExtraImports<'a>> {
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
    let mut imports: Vec<&crate::config::TsImport> = used
        .foreign
        .values()
        .filter_map(|foreign| foreign.macroforge.as_ref())
        .filter_map(|mapping| mapping.import.as_ref())
        .collect();
    let mut own_record_link = None;
    if used.record_link {
        match record_link_mapping(registry, OutputKind::Macroforge, |foreign| {
            foreign.macroforge.as_ref()
        })? {
            RecordLinkMapping::Configured(mapping) => imports.extend(mapping.import.as_ref()),
            RecordLinkMapping::Own { record_id } => {
                if declares_record_link {
                    imports.extend(record_id.import.as_ref());
                }
                own_record_link = Some(OwnRecordLink { record_id });
            }
        }
    }
    Ok(ExtraImports {
        lines: import_lines(imports),
        own_record_link,
    })
}

/// One validator as the macroforge output writes it: a validator macroforge
/// provides, or a function in the output's helpers module.
enum MacroforgeValidator {
    Native(String),
    Helper(HelperFunction),
}

/// What a helper function accepts before its check applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperInput {
    Text,
    Decimal,
    Duration,
}

/// A validator macroforge does not provide, as a function over `value`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HelperFunction {
    /// The function's name before any bound suffix, such as `isAlpha`.
    kind: &'static str,
    /// Whether the check bakes in a bound, so each bound needs its own function.
    bounded: bool,
    input: HelperInput,
    /// A boolean expression over `value`, valid once `input` holds.
    predicate: String,
}

impl HelperFunction {
    fn text(kind: &'static str, bounded: bool, check: Option<JsCheck>) -> Result<Self> {
        let predicate = match check {
            Some(JsCheck::Pattern { source, flags }) => {
                format!("{}.test(value)", js_checks::regexp(&source, &flags)?)
            }
            Some(JsCheck::Predicate(predicate)) => predicate,
            Some(JsCheck::Length(_)) | None => {
                return Err(EvenframeError::type_sync(format!(
                    "the macroforge helper `{kind}` has no string check"
                )));
            }
        };
        Ok(Self {
            kind,
            bounded,
            input: HelperInput::Text,
            predicate,
        })
    }

    /// The exported name: the kind, with a bounded check's hash appended so
    /// each bound keeps one stable function.
    fn name(&self) -> String {
        if self.bounded {
            let hash = blake3::hash(self.predicate.as_bytes()).to_hex();
            format!("{}_{}", self.kind, &hash.as_str()[..8])
        } else {
            self.kind.to_owned()
        }
    }

    fn declaration(&self, name: &str) -> String {
        let guard = match self.input {
            HelperInput::Text => "typeof value === \"string\" && ",
            HelperInput::Decimal => {
                "(typeof value === \"string\" || typeof value === \"number\" || typeof value === \"bigint\") && "
            }
            HelperInput::Duration => "",
        };
        format!(
            "export function {name}(value: unknown): boolean {{\n\treturn {guard}({});\n}}\n",
            self.predicate
        )
    }
}

/// The helpers module of one macroforge output: the functions its types'
/// validators name, and the import specifier those types reach it by.
#[derive(Debug, Clone)]
pub struct HelperModule {
    source: String,
    functions: BTreeMap<String, HelperFunction>,
}

impl HelperModule {
    /// A module the generated types import as `source`, such as `./helpers`.
    pub fn new(source: String) -> Self {
        Self {
            source,
            functions: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Takes in the functions another part of the same output named.
    pub fn merge(&mut self, other: HelperModule) {
        self.functions.extend(other.functions);
    }

    /// Adds `function` and returns the validator naming it.
    fn reference(&mut self, function: HelperFunction) -> String {
        let name = function.name();
        let reference = format!(
            "custom({{ function: \\\"{name}\\\", source: \\\"{}\\\" }})",
            escape_for_jsdoc(&self.source)
        );
        self.functions.insert(name, function);
        reference
    }

    /// The module's source, with the shared functions its helpers call.
    pub fn content(&self) -> String {
        let mut content = String::new();
        if self
            .functions
            .values()
            .any(|function| function.input == HelperInput::Decimal)
        {
            content.push_str(js_checks::COMPARE_DECIMAL);
        }
        if self
            .functions
            .values()
            .any(|function| function.input == HelperInput::Duration)
        {
            content.push_str(js_checks::DURATION_NANOS);
        }
        for (name, function) in &self.functions {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&function.declaration(name));
        }
        content
    }
}

/// A field's validators as the items of a `@endec({ validate: [...] })`
/// array, each a quoted string. A char field is held to exactly one
/// character.
fn collect_validators_for_field(
    validators: &[Validator],
    field_type: &FieldType,
    field_name: &str,
    helpers: &mut HelperModule,
) -> Result<String> {
    crate::validator::bounds::check_validators(validators)
        .map_err(|problem| EvenframeError::config(format!("field '{field_name}': {problem}")))?;
    let mut result = Vec::new();
    if matches!(field_type, FieldType::Char) {
        result.push(format!(
            "pattern({})",
            escape_for_jsdoc(js_checks::ONE_CHARACTER)
        ));
    }
    for validator in validators {
        match macroforge_validator(validator)? {
            Some(MacroforgeValidator::Native(native)) => result.push(native),
            Some(MacroforgeValidator::Helper(function)) => {
                result.push(helpers.reference(function));
            }
            None => {}
        }
    }
    Ok(result
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>()
        .join(", "))
}

/// A validator as the macroforge output writes it, or `None` for one that
/// checks nothing: a transform, a parse morph or a carrier, which a
/// macroforge type reads as already applied.
fn macroforge_validator(validator: &Validator) -> Result<Option<MacroforgeValidator>> {
    let native = |text: String| Ok(Some(MacroforgeValidator::Native(text)));
    let quoted = |text: &str| format!("\\\"{}\\\"", escape_for_jsdoc(text));
    match validator {
        Validator::StringValidator(string_validator) => {
            string_validator_to_macroforge(string_validator)
        }
        Validator::NumberValidator(number_validator) => native(match number_validator {
            NumberValidator::Int => "int".to_owned(),
            NumberValidator::Finite => "finite".to_owned(),
            NumberValidator::NonNaN => "nonNaN".to_owned(),
            NumberValidator::Positive => "positive".to_owned(),
            NumberValidator::Negative => "negative".to_owned(),
            NumberValidator::NonPositive => "nonPositive".to_owned(),
            NumberValidator::NonNegative => "nonNegative".to_owned(),
            NumberValidator::GreaterThan(bound) => format!("greaterThan({})", bound.0),
            NumberValidator::GreaterThanOrEqualTo(bound) => {
                format!("greaterThanOrEqualTo({})", bound.0)
            }
            NumberValidator::LessThan(bound) => format!("lessThan({})", bound.0),
            NumberValidator::LessThanOrEqualTo(bound) => format!("lessThanOrEqualTo({})", bound.0),
            NumberValidator::Between(start, end) => format!("between({}, {})", start.0, end.0),
            NumberValidator::MultipleOf(divisor) => format!("multipleOf({})", divisor.0),
            NumberValidator::Uint8 => "uint8".to_owned(),
        }),
        Validator::ArrayValidator(array_validator) => native(match array_validator {
            ArrayValidator::MinItems(count) => format!("minItems({count})"),
            ArrayValidator::MaxItems(count) => format!("maxItems({count})"),
            ArrayValidator::ItemsCount(count) => format!("itemsCount({count})"),
        }),
        Validator::DateValidator(date_validator) => native(match date_validator {
            DateValidator::ValidDate => "validDate".to_owned(),
            DateValidator::GreaterThanDate(bound) => format!("greaterThanDate({})", quoted(bound)),
            DateValidator::GreaterThanOrEqualToDate(bound) => {
                format!("greaterThanOrEqualToDate({})", quoted(bound))
            }
            DateValidator::LessThanDate(bound) => format!("lessThanDate({})", quoted(bound)),
            DateValidator::LessThanOrEqualToDate(bound) => {
                format!("lessThanOrEqualToDate({})", quoted(bound))
            }
            DateValidator::BetweenDate(start, end) => {
                format!("betweenDate({}, {})", quoted(start), quoted(end))
            }
        }),
        Validator::BigIntValidator(big_int_validator) => native(match big_int_validator {
            BigIntValidator::PositiveBigInt => "positiveBigInt".to_owned(),
            BigIntValidator::NegativeBigInt => "negativeBigInt".to_owned(),
            BigIntValidator::NonPositiveBigInt => "nonPositiveBigInt".to_owned(),
            BigIntValidator::NonNegativeBigInt => "nonNegativeBigInt".to_owned(),
            BigIntValidator::GreaterThanBigInt(bound) => {
                format!("greaterThanBigInt({})", quoted(bound))
            }
            BigIntValidator::GreaterThanOrEqualToBigInt(bound) => {
                format!("greaterThanOrEqualToBigInt({})", quoted(bound))
            }
            BigIntValidator::LessThanBigInt(bound) => format!("lessThanBigInt({})", quoted(bound)),
            BigIntValidator::LessThanOrEqualToBigInt(bound) => {
                format!("lessThanOrEqualToBigInt({})", quoted(bound))
            }
            BigIntValidator::BetweenBigInt(start, end) => {
                format!("betweenBigInt({}, {})", quoted(start), quoted(end))
            }
        }),
        Validator::BigDecimalValidator(decimal_validator) => {
            let (kind, bounded) = match decimal_validator {
                BigDecimalValidator::GreaterThanBigDecimal(_) => ("isGreaterThanBigDecimal", true),
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(_) => {
                    ("isGreaterThanOrEqualToBigDecimal", true)
                }
                BigDecimalValidator::LessThanBigDecimal(_) => ("isLessThanBigDecimal", true),
                BigDecimalValidator::LessThanOrEqualToBigDecimal(_) => {
                    ("isLessThanOrEqualToBigDecimal", true)
                }
                BigDecimalValidator::BetweenBigDecimal(_, _) => ("isBetweenBigDecimal", true),
                BigDecimalValidator::PositiveBigDecimal => ("isPositiveBigDecimal", false),
                BigDecimalValidator::NonNegativeBigDecimal => ("isNonNegativeBigDecimal", false),
                BigDecimalValidator::NegativeBigDecimal => ("isNegativeBigDecimal", false),
                BigDecimalValidator::NonPositiveBigDecimal => ("isNonPositiveBigDecimal", false),
            };
            Ok(Some(MacroforgeValidator::Helper(HelperFunction {
                kind,
                bounded,
                input: HelperInput::Decimal,
                predicate: js_checks::decimal_check(decimal_validator)?,
            })))
        }
        Validator::DurationValidator(duration_validator) => {
            let kind = match duration_validator {
                DurationValidator::GreaterThanDuration(_) => "isGreaterThanDuration",
                DurationValidator::GreaterThanOrEqualToDuration(_) => {
                    "isGreaterThanOrEqualToDuration"
                }
                DurationValidator::LessThanDuration(_) => "isLessThanDuration",
                DurationValidator::LessThanOrEqualToDuration(_) => "isLessThanOrEqualToDuration",
                DurationValidator::BetweenDuration(_, _) => "isBetweenDuration",
            };
            Ok(Some(MacroforgeValidator::Helper(HelperFunction {
                kind,
                bounded: true,
                input: HelperInput::Duration,
                predicate: js_checks::duration_check(duration_validator)?,
            })))
        }
    }
}

fn string_validator_to_macroforge(
    validator: &StringValidator,
) -> Result<Option<MacroforgeValidator>> {
    let native = |text: String| Ok(Some(MacroforgeValidator::Native(text)));
    let quoted = |text: &str| format!("\\\"{}\\\"", escape_for_jsdoc(text));
    let helper = |kind: &'static str, bounded: bool| -> Result<Option<MacroforgeValidator>> {
        Ok(Some(MacroforgeValidator::Helper(HelperFunction::text(
            kind,
            bounded,
            js_checks::string_check(validator)?,
        )?)))
    };
    match validator {
        StringValidator::MinLength(length) => native(format!("minLength({length})")),
        StringValidator::MaxLength(length) => native(format!("maxLength({length})")),
        StringValidator::Length(length) => native(format!("length({length})")),
        StringValidator::NonEmpty => native("nonEmpty".to_owned()),
        StringValidator::Email => native("email".to_owned()),
        StringValidator::Url => native("url".to_owned()),
        StringValidator::Uuid => native("uuid".to_owned()),
        StringValidator::Lowercased => native("lowercase".to_owned()),
        StringValidator::Uppercased => native("uppercase".to_owned()),
        StringValidator::Trimmed => native("trimmed".to_owned()),
        StringValidator::Capitalized => native("capitalized".to_owned()),
        StringValidator::Uncapitalized => native("uncapitalized".to_owned()),
        StringValidator::StartsWith(prefix) => native(format!("startsWith({})", quoted(prefix))),
        StringValidator::EndsWith(suffix) => native(format!("endsWith({})", quoted(suffix))),
        StringValidator::Includes(substring) => native(format!("includes({})", quoted(substring))),
        // Macroforge's `pattern` takes no flags, so a flagged pattern is checked
        // by a helper of its own.
        StringValidator::RegexLiteral(Format::Custom(custom))
            if custom.flags().is_some_and(|flags| !flags.is_empty()) =>
        {
            helper("matchesPattern", true)
        }
        StringValidator::RegexLiteral(format) => {
            native(format!("pattern({})", escape_for_jsdoc(&format.pattern())))
        }

        StringValidator::Alpha => helper("isAlpha", false),
        StringValidator::Alphanumeric => helper("isAlphanumeric", false),
        StringValidator::Base64 => helper("isBase64", false),
        StringValidator::Base64Url => helper("isBase64Url", false),
        StringValidator::CreditCard => helper("isCreditCard", false),
        StringValidator::Date => helper("isDate", false),
        StringValidator::DateIso => helper("isDateIso", false),
        StringValidator::DateEpoch => helper("isDateEpoch", false),
        StringValidator::Digits => helper("isDigits", false),
        StringValidator::Hex => helper("isHex", false),
        StringValidator::Integer => helper("isInteger", false),
        StringValidator::Numeric => helper("isNumeric", false),
        StringValidator::Ip => helper("isIp", false),
        StringValidator::IpV4 => helper("isIpV4", false),
        StringValidator::IpV6 => helper("isIpV6", false),
        StringValidator::Json => helper("isJson", false),
        StringValidator::Semver => helper("isSemver", false),
        StringValidator::Regex => helper("isRegex", false),
        StringValidator::UuidV1 => helper("isUuidV1", false),
        StringValidator::UuidV2 => helper("isUuidV2", false),
        StringValidator::UuidV3 => helper("isUuidV3", false),
        StringValidator::UuidV4 => helper("isUuidV4", false),
        StringValidator::UuidV5 => helper("isUuidV5", false),
        StringValidator::UuidV6 => helper("isUuidV6", false),
        StringValidator::UuidV7 => helper("isUuidV7", false),
        StringValidator::UuidV8 => helper("isUuidV8", false),
        StringValidator::LowerPreformatted => helper("isLowerPreformatted", false),
        StringValidator::UpperPreformatted => helper("isUpperPreformatted", false),
        StringValidator::TrimPreformatted => helper("isTrimPreformatted", false),
        StringValidator::CapitalizePreformatted => helper("isCapitalizePreformatted", false),
        StringValidator::NormalizeNFCPreformatted => helper("isNormalizedNfc", false),
        StringValidator::NormalizeNFDPreformatted => helper("isNormalizedNfd", false),
        StringValidator::NormalizeNFKCPreformatted => helper("isNormalizedNfkc", false),
        StringValidator::NormalizeNFKDPreformatted => helper("isNormalizedNfkd", false),
        StringValidator::Literal(_) => helper("isLiteral", true),

        StringValidator::String
        | StringValidator::StringEmbedded(_)
        | StringValidator::Capitalize
        | StringValidator::Lower
        | StringValidator::Upper
        | StringValidator::Trim
        | StringValidator::Normalize
        | StringValidator::NormalizeNFC
        | StringValidator::NormalizeNFD
        | StringValidator::NormalizeNFKC
        | StringValidator::NormalizeNFKD
        | StringValidator::DateParse
        | StringValidator::DateEpochParse
        | StringValidator::DateIsoParse
        | StringValidator::IntegerParse
        | StringValidator::NumericParse
        | StringValidator::JsonParse
        | StringValidator::UrlParse => Ok(None),
    }
}

/// Escapes text for a string inside a JSDoc `@endec(...)` annotation: its
/// quotes and backslashes, and a `*/` that would end the comment.
fn escape_for_jsdoc(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("*/", "*\\/")
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayStyle, ArrayValidator, BTreeMap, BigDecimalValidator, DurationValidator, FieldType,
        HelperModule, MacroforgeValidator, NumberValidator, Rendering, StringValidator,
        StructConfig, TaggedUnion, TypeIndex, Validator, collect_validators_for_field,
        compute_extra_imports, field_type_to_typescript, generate_macroforge_for_types,
        generate_macroforge_type_string, macro_import_lines, macroforge_validator,
    };
    use crate::types::{EnumRepresentation, NewtypeConfig, Pipeline, StructField, Variant};
    use ordered_float::OrderedFloat;

    #[test]
    fn a_flagged_pattern_is_checked_by_a_helper_with_its_flags() {
        use crate::schemasync::format::{CustomPattern, Format, PatternDialect};
        let pattern = |written: &str, dialect| {
            StringValidator::RegexLiteral(Format::Custom(
                CustomPattern::parse(written, dialect).expect("the pattern parses"),
            ))
        };
        match super::string_validator_to_macroforge(&pattern(
            r"/^\p{Lu}/u",
            PatternDialect::JavaScript,
        )) {
            Ok(Some(MacroforgeValidator::Helper(helper))) => {
                assert_eq!(helper.kind, "matchesPattern");
                assert_eq!(
                    helper.predicate,
                    r#"new RegExp("^\\p{Lu}", "u").test(value)"#
                );
            }
            _ => panic!("a flagged pattern needs a helper"),
        }
        match super::string_validator_to_macroforge(&pattern("^[a-z]+$", PatternDialect::Portable))
        {
            Ok(Some(MacroforgeValidator::Native(native))) => {
                assert!(native.starts_with("pattern("), "{native}");
            }
            _ => panic!("an unflagged pattern is macroforge's own"),
        }
    }

    /// `field_type` in TypeScript, with no scanned types beside it.
    fn typescript(
        field_type: &FieldType,
        array_style: ArrayStyle,
        registry: &crate::types::ForeignTypeRegistry,
    ) -> String {
        let structs = BTreeMap::new();
        let enums = BTreeMap::new();
        let index = TypeIndex::new(&structs, &enums).unwrap();
        field_type_to_typescript(
            field_type,
            Rendering {
                array_style,
                registry,
                index: &index,
                default_derives: None,
            },
        )
    }

    #[test]
    fn fields_follow_ts_policy_and_explicit_serde_names() {
        use crate::typesync::config::TsNames;
        use crate::typesync::naming::{apply_struct, wire_named_structs};
        for (policy, first_name) in [
            (TsNames::Default, "firstName"),
            (TsNames::RespectSerde, "first_name"),
        ] {
            let mut structs = wire_named_structs();
            for struct_config in structs.values_mut() {
                apply_struct(struct_config, policy).expect("apply TS naming policy");
            }
            let output = generate_macroforge_type_string(
                &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
                ArrayStyle::default(),
                &crate::types::ForeignTypeRegistry::default(),
                &mut helpers(),
                None,
            )
            .unwrap();
            assert!(
                output.contains(&format!("  {first_name}: string;")),
                "{output}"
            );
            assert!(output.contains("  lastName: string;"), "{output}");
            assert!(output.contains("  \"zip-code\": string;"), "{output}");
            assert!(output.contains("  nickname?: string;"), "{output}");
        }
    }

    /// The validator macroforge provides for `validator`, if it checks anything.
    fn native(validator: &Validator) -> Option<String> {
        match macroforge_validator(validator).expect("the validator maps") {
            Some(MacroforgeValidator::Native(text)) => Some(text),
            Some(MacroforgeValidator::Helper(_)) => panic!("{validator:?} should be native"),
            None => None,
        }
    }

    fn helpers() -> HelperModule {
        HelperModule::new("./helpers".to_owned())
    }

    fn collected(validators: &[Validator], field_type: &FieldType) -> String {
        collect_validators_for_field(validators, field_type, "field", &mut helpers())
            .expect("the validators collect")
    }

    #[test]
    fn validators_macroforge_lacks_become_helper_functions() {
        let mut module = helpers();
        let validators = vec![
            Validator::StringValidator(StringValidator::Alpha),
            Validator::StringValidator(StringValidator::Literal("a,\"b\"".to_owned())),
            Validator::BigDecimalValidator(BigDecimalValidator::GreaterThanBigDecimal(
                "1.5".to_owned(),
            )),
            Validator::DurationValidator(DurationValidator::LessThanDuration("1h".to_owned())),
            Validator::StringValidator(StringValidator::Alpha),
        ];
        let items =
            collect_validators_for_field(&validators, &FieldType::String, "field", &mut module)
                .expect("the validators collect");
        assert!(
            items.starts_with(
                "\"custom({ function: \\\"isAlpha\\\", source: \\\"./helpers\\\" })\""
            )
        );
        let content = module.content();
        assert_eq!(content.matches("export function isAlpha(").count(), 1);
        assert!(content.contains("const compareDecimal"));
        assert!(content.contains("const durationNanos"));
        assert!(content.contains("export function isLiteral_"));
        assert!(content.contains("value === \"a,\\\"b\\\"\""));
        assert!(content.contains("export function isGreaterThanBigDecimal_"));
        assert!(content.contains("export function isLessThanDuration_"));
    }

    #[test]
    fn a_bounded_helper_is_named_by_its_bound() {
        let mut module = helpers();
        let mut name_for = |bound: &str| {
            collect_validators_for_field(
                &[Validator::DurationValidator(
                    DurationValidator::GreaterThanDuration(bound.to_owned()),
                )],
                &FieldType::Duration,
                "field",
                &mut module,
            )
            .expect("the validators collect")
        };
        let one_hour = name_for("1h");
        assert_eq!(one_hour, name_for("1h"));
        assert_ne!(one_hour, name_for("2h"));
    }

    #[test]
    fn a_bad_bound_is_rejected() {
        let result = collect_validators_for_field(
            &[Validator::DurationValidator(
                DurationValidator::GreaterThanDuration("soon".to_owned()),
            )],
            &FieldType::Duration,
            "timeout",
            &mut helpers(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_string_validators_to_macroforge() {
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Email)),
            Some("email".to_string())
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::MinLength(8))),
            Some("minLength(8)".to_string())
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::MaxLength(50))),
            Some("maxLength(50)".to_string())
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Uuid)),
            Some("uuid".to_string())
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Lowercased)),
            Some("lowercase".to_string())
        );
    }

    #[test]
    fn test_number_validators_to_macroforge() {
        assert_eq!(
            native(&Validator::NumberValidator(NumberValidator::Int)),
            Some("int".to_string())
        );
        assert_eq!(
            native(&Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(18.0),
                OrderedFloat(120.0)
            ))),
            Some("between(18, 120)".to_string())
        );
        assert_eq!(
            native(&Validator::NumberValidator(NumberValidator::Positive)),
            Some("positive".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_macroforge() {
        assert_eq!(
            native(&Validator::ArrayValidator(ArrayValidator::MinItems(1))),
            Some("minItems(1)".to_string())
        );
        assert_eq!(
            native(&Validator::ArrayValidator(ArrayValidator::MaxItems(5))),
            Some("maxItems(5)".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_skipped() {
        // These should return None as they're transformations, not validations
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Lower)),
            None
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Upper)),
            None
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::Trim)),
            None
        );
        assert_eq!(
            native(&Validator::StringValidator(StringValidator::IntegerParse)),
            None
        );
    }

    #[test]
    fn test_field_type_to_typescript() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let s = ArrayStyle::Shorthand;
        assert_eq!(typescript(&FieldType::String, s, &registry), "string");
        assert_eq!(typescript(&FieldType::Bool, s, &registry), "boolean");
        assert_eq!(typescript(&FieldType::I32, s, &registry), "number");
        assert_eq!(typescript(&FieldType::F64, s, &registry), "number");
        assert!(
            typescript(
                &FieldType::Option(Box::new(FieldType::String)),
                s,
                &registry
            )
            .contains("string")
                && typescript(
                    &FieldType::Option(Box::new(FieldType::String)),
                    s,
                    &registry
                )
                .contains("null")
        );
        let vec_output = typescript(&FieldType::Vec(Box::new(FieldType::I32)), s, &registry);
        assert!(vec_output.contains("number") && vec_output.contains("[]"));
        assert!(
            typescript(&FieldType::Other("UserProfile".to_string()), s, &registry)
                .contains("UserProfile")
        );
    }

    #[test]
    fn test_field_type_to_typescript_generic_array_style() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let g = ArrayStyle::Generic;
        // Vec<i32> → Array<number>
        let vec_output = typescript(&FieldType::Vec(Box::new(FieldType::I32)), g, &registry);
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
        let vec_opt = typescript(
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
        let render =
            |field_type: FieldType, style: ArrayStyle| typescript(&field_type, style, &registry);
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
    fn a_string_field_carries_only_its_declared_validators() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        assert_eq!(
            collected(&validators, &FieldType::String),
            "\"email\", \"minLength(5)\""
        );
        assert_eq!(collected(&[], &FieldType::String), "");
    }

    #[test]
    fn a_number_field_carries_its_declared_validators() {
        let validators = vec![Validator::NumberValidator(NumberValidator::Int)];
        assert_eq!(collected(&validators, &FieldType::I32), "\"int\"");
    }

    #[test]
    fn test_collect_validators_skips_transformations() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::Lower), // Should be skipped
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        assert_eq!(
            collected(&validators, &FieldType::String),
            "\"email\", \"minLength(5)\""
        );
    }

    #[test]
    fn an_explicit_non_empty_is_written() {
        let validators = vec![
            Validator::StringValidator(StringValidator::NonEmpty),
            Validator::StringValidator(StringValidator::Email),
        ];
        assert_eq!(
            collected(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\""
        );
    }

    #[test]
    fn foreign_format_config_emits_an_endec_annotation() {
        let foreign: crate::config::ForeignTypeConfig = toml::from_str(
            r#"
                endec_format = "decimal"
                macroforge = { type = "number" }
            "#,
        )
        .unwrap();
        let registry = crate::types::ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "Counter".to_string(),
            foreign,
        )]));
        let structs = BTreeMap::from([(
            "Reading".to_string(),
            StructConfig {
                struct_name: "Reading".to_string(),
                fields: vec![StructField {
                    field_name: "count".to_string(),
                    field_type: FieldType::Other("Counter".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        )]);
        let output = generate_macroforge_type_string(
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            ArrayStyle::default(),
            &registry,
            &mut helpers(),
            None,
        )
        .unwrap();
        assert!(output.contains("/** @endec({ format: \"decimal\" }) */"));
        assert!(output.contains("count: number;"));
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
            &mut helpers(),
            None,
        )
        .unwrap();

        assert!(output.contains("/** @derive(Decode) */"));
        assert!(output.contains("export interface UserRegistrationForm"));
        assert!(output.contains("@endec({ validate: [\"email\"] })"));
        assert!(output.contains("@endec({ validate: [\"minLength(8)\", \"maxLength(50)\"] })"));
        assert!(output.contains("@endec({ validate: [\"int\", \"between(18, 120)\"] })"));
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
                            "@input({ label: \"Title\" })".to_string(),
                            "@column({ heading: \"Title\" })".to_string(),
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
                    "Encode".to_string(),
                    "Decode".to_string(),
                    "Form".to_string(),
                    "Listing".to_string(),
                ],
                annotations: vec![
                    "@listing({ dataName: \"account\", apiUrl: \"/api/accounts\" })".to_string(),
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
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec!["@default".to_string()],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "OnDeck".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                ],
                doccom: None,
                macroforge_derives: vec![
                    "Default".to_string(),
                    "Encode".to_string(),
                    "Decode".to_string(),
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
            &mut helpers(),
            None,
        )
        .unwrap();

        // Struct: custom derives
        assert!(
            output.contains("/** @derive(Default, Encode, Decode, Form, Listing) */"),
            "Should contain custom derives. Output:\n{}",
            output
        );
        // Struct: type-level annotation
        assert!(
            output
                .contains("/** @listing({ dataName: \"account\", apiUrl: \"/api/accounts\" }) */"),
            "Should contain struct-level annotation. Output:\n{}",
            output
        );
        // Struct: field-level annotations
        assert!(
            output.contains("/** @input({ label: \"Title\" }) */"),
            "Should contain field-level input annotation. Output:\n{}",
            output
        );
        assert!(
            output.contains("/** @column({ heading: \"Title\" }) */"),
            "Should contain field-level column annotation. Output:\n{}",
            output
        );

        // Enum: custom derives
        assert!(
            output.contains("/** @derive(Default, Encode, Decode) */"),
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
    fn test_empty_macroforge_derives_falls_back_to_decode() {
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
            &mut helpers(),
            None,
        )
        .unwrap();
        assert!(
            output.contains("/** @derive(Decode) */"),
            "Empty macroforge_derives should fall back to Decode. Output:\n{}",
            output
        );
    }

    #[test]
    fn default_derives_apply_to_every_type_without_its_own() {
        let structs = BTreeMap::from([
            (
                "plain".to_owned(),
                StructConfig {
                    struct_name: "plain".to_owned(),
                    ..Default::default()
                },
            ),
            (
                "chosen".to_owned(),
                StructConfig {
                    struct_name: "chosen".to_owned(),
                    macroforge_derives: vec!["Encode".to_owned()],
                    ..Default::default()
                },
            ),
        ]);
        let newtypes = BTreeMap::from([(
            "Slug".to_owned(),
            NewtypeConfig {
                name: "Slug".to_owned(),
                inner: FieldType::String,
                ..Default::default()
            },
        )]);
        let enums = BTreeMap::new();
        let index = TypeIndex::with_newtypes(&structs, &enums, &newtypes).unwrap();
        let registry = crate::types::ForeignTypeRegistry::default();
        let generate = |default_derives: Option<&[String]>| {
            generate_macroforge_type_string(
                &index,
                ArrayStyle::default(),
                &registry,
                &mut helpers(),
                default_derives,
            )
            .unwrap()
        };

        let defaults = ["Default".to_owned(), "Form".to_owned(), "Decode".to_owned()];
        let configured = generate(Some(&defaults));
        assert!(
            configured.contains("/** @derive(Default, Form, Decode) */\nexport interface Plain"),
            "{configured}"
        );
        assert!(
            configured.contains("/** @derive(Default, Form, Decode) */\nexport type Slug"),
            "{configured}"
        );
        assert!(
            configured.contains("/** @derive(Encode) */\nexport interface Chosen"),
            "a type's own derives win: {configured}"
        );

        // Unset, a struct falls back to `Decode` and a newtype gets none.
        let unset = generate(None);
        assert!(
            unset.contains("/** @derive(Decode) */\nexport interface Plain"),
            "{unset}"
        );
        assert!(
            !unset.contains("*/\nexport type Slug"),
            "a newtype with no derives has no derive line: {unset}"
        );

        let types = ["plain".to_owned(), "Slug".to_owned()];
        let macros = BTreeMap::from([("Form".to_owned(), "@app/forms".to_owned())]);
        assert_eq!(
            macro_import_lines(&types, &index, &macros, Some(&defaults)).unwrap(),
            vec!["/** import macro {Form} from \"@app/forms\"; */".to_owned()],
            "a default derive is imported like a declared one"
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
            typescript(&FieldType::Other("DateTime".to_string()), s, &registry)
                .contains("DateTime.Utc")
        );
        // Option<DateTime> should produce DateTime.Utc | null
        let opt_dt = typescript(
            &FieldType::Option(Box::new(FieldType::Other("DateTime".to_string()))),
            s,
            &registry,
        );
        assert!(opt_dt.contains("DateTime.Utc") && opt_dt.contains("null"));
        // Vec<DateTime> should produce DateTime.Utc[]
        let vec_dt = typescript(
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
            typescript(&FieldType::Other("Decimal".to_string()), s, &registry)
                .contains("BigDecimal.BigDecimal")
        );
        // Option<Decimal> should produce BigDecimal.BigDecimal | null
        let opt_dec = typescript(
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
            false,
        )
        .unwrap();
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
            false,
        )
        .unwrap();
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
                derived("Order", &["Debug", "Form", "Encode", "Decode"]),
            ),
            (
                "Invoice".to_string(),
                derived("Invoice", &["Listing", "Form", "Audit"]),
            ),
        ]);
        let types = ["Order".to_string(), "Invoice".to_string()];
        let macros = BTreeMap::from([
            ("Form".to_string(), "@app/forms".to_string()),
            ("Listing".to_string(), "@app/forms".to_string()),
            ("Audit".to_string(), "@app/audit".to_string()),
        ]);
        assert_eq!(
            macro_import_lines(
                &types,
                &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
                &macros,
                None,
            )
            .unwrap(),
            vec![
                "/** import macro {Audit} from \"@app/audit\"; */".to_string(),
                "/** import macro {Listing, Form} from \"@app/forms\"; */".to_string(),
            ]
        );
        let unconfigured = macro_import_lines(
            &types,
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &BTreeMap::new(),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            unconfigured.contains("`Listing`, `Form`, `Audit`") && !unconfigured.contains("Debug"),
            "{unconfigured}"
        );
    }

    #[test]
    fn imports_follow_embedded_types_but_not_tables() {
        use crate::config::{ForeignTypeConfig, TsMapping};
        let mut foreign_types = make_datetime_registry().configs().clone();
        foreign_types.insert(
            "RecordId".to_string(),
            ForeignTypeConfig {
                macroforge: Some(TsMapping {
                    type_expr: "string".to_string(),
                    import: None,
                }),
                ..Default::default()
            },
        );
        let registry = crate::types::ForeignTypeRegistry::from_config(&foreign_types);
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
                false,
            )
            .unwrap()
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
            false,
        )
        .unwrap();
        assert_eq!(
            imports.lines,
            vec!["import type { RecordLink } from './index';".to_string()]
        );
        assert!(imports.own_record_link.is_none());
        let unconfigured = crate::types::ForeignTypeRegistry::default();
        let without_entry = compute_extra_imports(
            &["Order".to_string()],
            &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
            &unconfigured,
            false,
        );
        assert!(without_entry.is_err());
    }

    #[test]
    fn the_own_record_link_names_the_configured_record_id() {
        use crate::config::{ForeignTypeConfig, TsImport, TsMapping};
        let registry = crate::types::ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "RecordId".to_string(),
            ForeignTypeConfig {
                macroforge: Some(TsMapping {
                    type_expr: "RecordIdEncoded".to_string(),
                    import: Some(TsImport {
                        from: "../record-id.ts".to_string(),
                        name: "RecordIdEncoded".to_string(),
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
        let enums = BTreeMap::new();
        let index = TypeIndex::new(&structs, &enums).unwrap();
        let declaring =
            compute_extra_imports(&["Order".to_string()], &index, &registry, true).unwrap();
        assert_eq!(
            declaring.lines,
            vec!["import type { RecordIdEncoded } from '../record-id.ts';".to_string()]
        );
        assert_eq!(
            declaring.own_record_link.map(|own| own.declaration()),
            Some("export type RecordLink<T> = RecordIdEncoded | T;".to_string())
        );
        let importing =
            compute_extra_imports(&["Order".to_string()], &index, &registry, false).unwrap();
        assert!(importing.lines.is_empty());
        assert!(importing.own_record_link.is_some());
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
            false,
        )
        .unwrap();
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
            false,
        )
        .unwrap();
        assert!(imports.lines.is_empty());
        assert!(imports.own_record_link.is_none());
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
                macroforge_derives: vec!["Encode".to_string(), "Decode".to_string()],
                annotations: vec!["@listing({ dataName: \"order\" })".to_string()],
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
            &mut helpers(),
            None,
        )
        .expect("the types render");

        assert!(
            output.contains("/** @derive(Encode, Decode) */"),
            "Should contain custom derives in per-file mode. Output:\n{}",
            output
        );
        assert!(
            output.contains("/** @listing({ dataName: \"order\" }) */"),
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
