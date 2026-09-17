use quote::quote;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, ExprArray, ExprLit, Ident, Lit, LitStr, Meta, Token, parenthesized,
    spanned::Spanned,
};
use tracing::{debug, error, info, trace};

use crate::{
    schemasync::{
        Bm25, Direction, EdgeConfig, IndexConfig, IndexKind, VectorDistance, VectorType,
        mockmake::{MockGenerationConfig, coordinate::Coordination, format::Format},
    },
    types::{EnumRepresentation, StructField},
};
use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;

// Remove unused imports - these are only used in the macro implementation, not generated code

pub fn parse_mock_data_attribute(
    attrs: &[Attribute],
) -> Result<Option<MockGenerationConfig>, syn::Error> {
    info!(
        "Starting mock_data attribute parsing for {} attributes",
        attrs.len()
    );
    trace!(
        "Processing attributes: {:?}",
        attrs
            .iter()
            .map(|a| a
                .path()
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "unknown".to_string()))
            .collect::<Vec<_>>()
    );

    for (index, attr) in attrs.iter().enumerate() {
        trace!("Processing attribute {} of {}", index + 1, attrs.len());
        if attr.path().is_ident("mock_data") {
            debug!("Found mock_data attribute, parsing arguments");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    debug!("Successfully parsed {} meta arguments", metas.len());
                    // Start with defaults from MockGenerationConfig::default()
                    let mut base_config = MockGenerationConfig::default();
                    let mut overrides_name = None;

                    for (meta_index, meta) in metas.iter().enumerate() {
                        trace!("Processing meta {} of {}", meta_index + 1, metas.len());
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("n") => {
                                debug!("Processing 'n' parameter");
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Int(lit), ..
                                }) = &nv.value
                                {
                                    match lit.base10_parse::<usize>() {
                                        Ok(value) => {
                                            debug!("Successfully parsed n value: {}", value);
                                            base_config.n = value;
                                        }
                                        Err(_) => {
                                            error!(
                                                "Failed to parse 'n' value: {}",
                                                lit.base10_digits()
                                            );
                                            return Err(syn::Error::new(
                                                lit.span(),
                                                format!(
                                                    "Invalid value for 'n': '{}'. Expected a positive integer.\n\nExample: #[mock_data(n = 1000)]",
                                                    lit.base10_digits()
                                                ),
                                            ));
                                        }
                                    }
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'n' parameter must be an integer literal.\n\nExample: #[mock_data(n = 1000)]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("overrides") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    overrides_name = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'overrides' parameter must be a string literal.\n\nExample: #[mock_data(overrides = \"custom_config\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("coordinate") => {
                                // Skip here - coordinate is parsed separately by coordinate_parser
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("plugin") => {
                                debug!("Processing 'plugin' parameter");
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    base_config.plugin = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'plugin' parameter must be a string literal.\n\nExample: #[mock_data(plugin = \"my_plugin\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in mock_data attribute.\n\nValid parameters are: n, overrides, coordinate, plugin\n\nExample: #[mock_data(n = 1000, plugin = \"my_plugin\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in mock_data attribute.\n\nExpected format: #[mock_data(n = 1000, overrides = \"config\")]",
                                ));
                            }
                        }
                    }

                    // Parse coordination rules directly from the attributes
                    let mut coordination_rules = Vec::new();

                    // Look for coordinate parameter in the metas we already have
                    for meta in metas.iter() {
                        if let Meta::NameValue(nv) = meta
                            && nv.path.is_ident("coordinate")
                        {
                            // coordinate = [...]
                            if let Expr::Array(ExprArray { elems, .. }) = &nv.value {
                                for elem in elems {
                                    match Coordination::try_from(elem) {
                                        Ok(coord) => {
                                            debug!(
                                                "Successfully parsed coordination rule: {:?}",
                                                coord
                                            );
                                            coordination_rules.push(coord);
                                        }
                                        Err(e) => {
                                            error!("Failed to parse coordination rule: {}", e);
                                            return Err(syn::Error::new(
                                                elem.span(),
                                                format!("Failed to parse coordination rule: {}", e),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    info!(
                        "Successfully parsed mock_data attribute: n={}, overrides={:?}, coordination_rules_count={}",
                        base_config.n,
                        overrides_name,
                        coordination_rules.len()
                    );

                    // Parse overrides from config if specified
                    let table_level_override: Option<HashMap<StructField, Format>> =
                        if let Some(override_name) = overrides_name {
                            // Loading format overrides from config is not currently supported.
                            // This code runs inside a proc macro (compile time), so it cannot
                            // access runtime config. Compile-time file reading is possible but
                            // fragile (cargo doesn't track TOML changes as dependencies).
                            // For now, specify format overrides inline via field-level attributes.
                            debug!(
                                "Override '{}' specified but override loading not yet implemented",
                                override_name
                            );
                            None
                        } else {
                            None
                        };

                    // Apply parsed values to the base config
                    base_config.table_level_override = table_level_override;
                    base_config.coordination_rules = coordination_rules;

                    return Ok(Some(base_config));
                }
                Err(err) => {
                    error!("Failed to parse mock_data attribute arguments: {}", err);
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse mock_data attribute: {}\n\nExample usage:\n#[mock_data(n = 1000)]\n#[mock_data(n = 500, overrides = \"custom_config\")]",
                            err
                        ),
                    ));
                }
            }
        }
    }
    debug!("No mock_data attribute found");
    Ok(None)
}

pub fn parse_event_attributes(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    info!(
        "Starting event attribute parsing for {} attributes",
        attrs.len()
    );

    let mut events = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("event") {
            debug!("Found event attribute");
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse event attribute: {}\n\nExpected usage: #[event(\"DEFINE EVENT name ON TABLE table WHEN ... THEN ...\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();
            trace!(event_statement = %value, "Parsed event attribute");

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Event statement cannot be empty.\n\nExample: #[event(\"DEFINE EVENT my_event ON TABLE user WHEN $before != $after THEN ...\")]",
                ));
            }

            events.push(value);
        }
    }

    debug!(
        event_count = events.len(),
        "Completed event attribute parsing"
    );
    Ok(events)
}

/// One entry of `fields(...)` in an `#[indexes(...)]` entry: a struct field identifier
/// (`user`) or a string path rooted at a struct field (`"tags.*"`).
enum IndexFieldEntry {
    Ident(Ident),
    Path(LitStr),
}

impl Parse for IndexFieldEntry {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            input.parse().map(IndexFieldEntry::Path)
        } else {
            input.parse().map(IndexFieldEntry::Ident)
        }
    }
}

const INDEX_ATTR_HELP: &str = "expected one of `fields(...)`, `unique`, `count`, \
     `comment = \"...\"` or `concurrently` in an #[indexes(...)] entry";

/// Field-level index attributes. Each covers exactly one field, which is why
/// they live on the field rather than in a struct-level `#[indexes(...)]`.
pub const FIELD_INDEX_ATTRIBUTES: [&str; 3] = ["fulltext", "hnsw", "diskann"];

fn index_lit_str(meta: &syn::meta::ParseNestedMeta) -> syn::Result<LitStr> {
    meta.value()?.parse::<LitStr>()
}

fn index_int<T>(meta: &syn::meta::ParseNestedMeta) -> syn::Result<(T, proc_macro2::Span)>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let lit = meta.value()?.parse::<syn::LitInt>()?;
    Ok((lit.base10_parse::<T>()?, lit.span()))
}

fn index_float(meta: &syn::meta::ParseNestedMeta) -> syn::Result<f64> {
    let lit = meta.value()?.parse::<Lit>()?;
    match &lit {
        Lit::Float(f) => f.base10_parse::<f64>(),
        Lit::Int(i) => i.base10_parse::<f64>(),
        _ => Err(syn::Error::new(lit.span(), "expected a number")),
    }
}

fn index_distance(meta: &syn::meta::ParseNestedMeta) -> syn::Result<(VectorDistance, LitStr)> {
    let lit = index_lit_str(meta)?;
    let dist = lit
        .value()
        .parse::<VectorDistance>()
        .map_err(|e| syn::Error::new(lit.span(), e))?;
    Ok((dist, lit))
}

fn index_vector_type(meta: &syn::meta::ParseNestedMeta) -> syn::Result<(VectorType, LitStr)> {
    let lit = index_lit_str(meta)?;
    let vector_type = lit
        .value()
        .parse::<VectorType>()
        .map_err(|e| syn::Error::new(lit.span(), e))?;
    Ok((vector_type, lit))
}

fn reject_duplicate<T>(
    slot: &Option<T>,
    meta: &syn::meta::ParseNestedMeta,
    key: &str,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate `{key}`")));
    }
    Ok(())
}

fn is_valid_index_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `name = "..."`, `comment = "..."` and `concurrently`, accepted by
/// `#[indexes(...)]` entries (except `name`) and by every field-level index attribute.
#[derive(Default)]
struct IndexModifiers {
    name: Option<String>,
    comment: Option<String>,
    concurrently: bool,
}

impl IndexModifiers {
    /// Consume `meta` if it is a modifier; returns whether it was one.
    fn accept(&mut self, meta: &syn::meta::ParseNestedMeta) -> syn::Result<bool> {
        if meta.path.is_ident("name") {
            reject_duplicate(&self.name, meta, "name")?;
            let lit = index_lit_str(meta)?;
            if !is_valid_index_name(&lit.value()) {
                return Err(syn::Error::new(
                    lit.span(),
                    "index `name` must be a valid SurrealQL identifier \
                     (letters, digits and `_`, not starting with a digit)",
                ));
            }
            self.name = Some(lit.value());
        } else if meta.path.is_ident("comment") {
            reject_duplicate(&self.comment, meta, "comment")?;
            self.comment = Some(index_lit_str(meta)?.value());
        } else if meta.path.is_ident("concurrently") {
            self.concurrently = true;
        } else {
            return Ok(false);
        }
        Ok(true)
    }
}

fn parse_fulltext_attribute(
    attr: &Attribute,
    modifiers: &mut IndexModifiers,
) -> syn::Result<IndexKind> {
    let mut analyzer: Option<String> = None;
    let mut bm25: Option<Bm25> = None;
    let mut highlights = false;
    // A bare `#[fulltext]` uses SurrealDB's defaults
    if matches!(attr.meta, Meta::Path(_)) {
        return Ok(IndexKind::FullText {
            analyzer,
            bm25,
            highlights,
        });
    }
    attr.parse_nested_meta(|inner| {
        if modifiers.accept(&inner)? {
            return Ok(());
        }
        if inner.path.is_ident("analyzer") {
            reject_duplicate(&analyzer, &inner, "analyzer")?;
            let lit = index_lit_str(&inner)?;
            if !is_valid_index_name(&lit.value()) {
                return Err(syn::Error::new(
                    lit.span(),
                    "`analyzer` must be a valid SurrealQL identifier",
                ));
            }
            analyzer = Some(lit.value());
        } else if inner.path.is_ident("bm25") {
            reject_duplicate(&bm25, &inner, "bm25")?;
            if inner.input.peek(syn::token::Paren) {
                let mut k1: Option<f32> = None;
                let mut b: Option<f32> = None;
                inner.parse_nested_meta(|param| {
                    if param.path.is_ident("k1") {
                        reject_duplicate(&k1, &param, "k1")?;
                        k1 = Some(index_float(&param)? as f32);
                    } else if param.path.is_ident("b") {
                        reject_duplicate(&b, &param, "b")?;
                        b = Some(index_float(&param)? as f32);
                    } else {
                        return Err(param.error("expected `k1 = <number>` or `b = <number>`"));
                    }
                    Ok(())
                })?;
                match (k1, b) {
                    (Some(k1), Some(b)) => bm25 = Some(Bm25::Params { k1, b }),
                    _ => {
                        return Err(inner.error(
                            "`bm25(...)` needs both `k1` and `b`, e.g. `bm25(k1 = 1.2, b = 0.75)`; \
                             use plain `bm25` for the defaults",
                        ));
                    }
                }
            } else {
                bm25 = Some(Bm25::Default);
            }
        } else if inner.path.is_ident("highlights") {
            highlights = true;
        } else {
            return Err(inner.error(
                "expected one of `analyzer = \"...\"`, `bm25`, `bm25(k1 = .., b = ..)`, \
                 `highlights`, `name = \"...\"`, `comment = \"...\"` or `concurrently` \
                 inside #[fulltext(...)]",
            ));
        }
        Ok(())
    })?;
    Ok(IndexKind::FullText {
        analyzer,
        bm25,
        highlights,
    })
}

fn parse_hnsw_attribute(
    attr: &Attribute,
    modifiers: &mut IndexModifiers,
) -> syn::Result<IndexKind> {
    let mut dimension: Option<u16> = None;
    let mut dist: Option<VectorDistance> = None;
    let mut vector_type: Option<VectorType> = None;
    let mut efc: Option<u16> = None;
    let mut m: Option<u8> = None;
    let mut m0: Option<u8> = None;
    let mut lm: Option<f64> = None;
    let mut extend_candidates = false;
    let mut keep_pruned_connections = false;
    let mut hashed_vector = false;
    if !matches!(attr.meta, Meta::Path(_)) {
        attr.parse_nested_meta(|inner| {
            if modifiers.accept(&inner)? {
                return Ok(());
            }
            if inner.path.is_ident("dimension") {
                reject_duplicate(&dimension, &inner, "dimension")?;
                dimension = Some(index_int(&inner)?.0);
            } else if inner.path.is_ident("dist") {
                reject_duplicate(&dist, &inner, "dist")?;
                dist = Some(index_distance(&inner)?.0);
            } else if inner.path.is_ident("type") {
                reject_duplicate(&vector_type, &inner, "type")?;
                vector_type = Some(index_vector_type(&inner)?.0);
            } else if inner.path.is_ident("efc") {
                reject_duplicate(&efc, &inner, "efc")?;
                efc = Some(index_int(&inner)?.0);
            } else if inner.path.is_ident("m") {
                reject_duplicate(&m, &inner, "m")?;
                let (value, span) = index_int::<u8>(&inner)?;
                if value > 127 {
                    return Err(syn::Error::new(span, "HNSW `m` cannot be larger than 127"));
                }
                m = Some(value);
            } else if inner.path.is_ident("m0") {
                reject_duplicate(&m0, &inner, "m0")?;
                m0 = Some(index_int(&inner)?.0);
            } else if inner.path.is_ident("lm") {
                reject_duplicate(&lm, &inner, "lm")?;
                lm = Some(index_float(&inner)?);
            } else if inner.path.is_ident("extend_candidates") {
                extend_candidates = true;
            } else if inner.path.is_ident("keep_pruned_connections") {
                keep_pruned_connections = true;
            } else if inner.path.is_ident("hashed_vector") {
                hashed_vector = true;
            } else {
                return Err(inner.error(
                    "expected one of `dimension`, `dist`, `type`, `efc`, `m`, `m0`, `lm`, \
                     `extend_candidates`, `keep_pruned_connections`, `hashed_vector`, \
                     `name = \"...\"`, `comment = \"...\"` or `concurrently` inside #[hnsw(...)]",
                ));
            }
            Ok(())
        })?;
    }
    let dimension = dimension.ok_or_else(|| {
        syn::Error::new(
            attr.path().span(),
            "#[hnsw(...)] requires `dimension = <n>`, e.g. `#[hnsw(dimension = 1536)]`",
        )
    })?;
    Ok(IndexKind::Hnsw {
        dimension,
        dist,
        vector_type,
        efc,
        m,
        m0,
        lm,
        extend_candidates,
        keep_pruned_connections,
        hashed_vector,
    })
}

fn parse_diskann_attribute(
    attr: &Attribute,
    modifiers: &mut IndexModifiers,
) -> syn::Result<IndexKind> {
    let mut dimension: Option<u16> = None;
    let mut dist: Option<(VectorDistance, LitStr)> = None;
    let mut vector_type: Option<(VectorType, LitStr)> = None;
    let mut degree: Option<u32> = None;
    let mut l_build: Option<u32> = None;
    let mut alpha: Option<f64> = None;
    let mut hashed_vector = false;
    if !matches!(attr.meta, Meta::Path(_)) {
        attr.parse_nested_meta(|inner| {
            if modifiers.accept(&inner)? {
                return Ok(());
            }
            if inner.path.is_ident("dimension") {
                reject_duplicate(&dimension, &inner, "dimension")?;
                dimension = Some(index_int(&inner)?.0);
            } else if inner.path.is_ident("dist") {
                reject_duplicate(&dist, &inner, "dist")?;
                let (value, lit) = index_distance(&inner)?;
                if !value.supported_by_diskann() {
                    return Err(syn::Error::new(
                        lit.span(),
                        "DISKANN supports `dist` euclidean, cosine, inner_product and cosine_normalized",
                    ));
                }
                dist = Some((value, lit));
            } else if inner.path.is_ident("type") {
                reject_duplicate(&vector_type, &inner, "type")?;
                let (value, lit) = index_vector_type(&inner)?;
                if !value.supported_by_diskann() {
                    return Err(syn::Error::new(
                        lit.span(),
                        "DISKANN supports `type` f32, f16, i8 and u8",
                    ));
                }
                vector_type = Some((value, lit));
            } else if inner.path.is_ident("degree") {
                reject_duplicate(&degree, &inner, "degree")?;
                let (value, span) = index_int::<u32>(&inner)?;
                if value == 0 {
                    return Err(syn::Error::new(span, "DISKANN `degree` must be greater than 0"));
                }
                degree = Some(value);
            } else if inner.path.is_ident("l_build") {
                reject_duplicate(&l_build, &inner, "l_build")?;
                let (value, span) = index_int::<u32>(&inner)?;
                if value == 0 {
                    return Err(syn::Error::new(span, "DISKANN `l_build` must be greater than 0"));
                }
                l_build = Some(value);
            } else if inner.path.is_ident("alpha") {
                reject_duplicate(&alpha, &inner, "alpha")?;
                alpha = Some(index_float(&inner)?);
            } else if inner.path.is_ident("hashed_vector") {
                hashed_vector = true;
            } else {
                return Err(inner.error(
                    "expected one of `dimension`, `dist`, `type`, `degree`, `l_build`, `alpha`, \
                     `hashed_vector`, `name = \"...\"`, `comment = \"...\"` or `concurrently` \
                     inside #[diskann(...)]",
                ));
            }
            Ok(())
        })?;
    }
    let dimension = dimension.ok_or_else(|| {
        syn::Error::new(
            attr.path().span(),
            "#[diskann(...)] requires `dimension = <n>`, e.g. `#[diskann(dimension = 1536)]`",
        )
    })?;
    if let (
        Some((VectorDistance::CosineNormalized, _)),
        Some((VectorType::I8 | VectorType::U8, lit)),
    ) = (&dist, &vector_type)
    {
        return Err(syn::Error::new(
            lit.span(),
            "DISKANN with `dist = \"cosine_normalized\"` supports `type` f32 and f16 only",
        ));
    }
    Ok(IndexKind::DiskAnn {
        dimension,
        dist: dist.map(|(value, _)| value),
        vector_type: vector_type.map(|(value, _)| value),
        degree,
        l_build,
        alpha,
        hashed_vector,
    })
}

fn parse_unique_attribute(
    attr: &Attribute,
    modifiers: &mut IndexModifiers,
) -> syn::Result<IndexKind> {
    // A bare `#[unique]` is the common case
    if !matches!(attr.meta, Meta::Path(_)) {
        attr.parse_nested_meta(|inner| {
            if modifiers.accept(&inner)? {
                return Ok(());
            }
            Err(inner.error(
                "expected `name = \"...\"`, `comment = \"...\"` or `concurrently` inside #[unique(...)]",
            ))
        })?;
    }
    Ok(IndexKind::Unique)
}

/// A parsed index and the span its problems are reported on.
pub type SpannedIndex = (IndexConfig, proc_macro2::Span);

/// Parses the field-level index attributes (`#[unique]`, `#[fulltext(...)]`,
/// `#[hnsw(...)]`, `#[diskann(...)]`) on the field named `field_name` (with
/// any `r#` prefix already stripped) into one `IndexConfig` each.
///
/// A field carries at most one attribute of each kind. Different kinds never
/// repeat an attribute item, so `clippy::duplicated_attributes` can't fire.
///
/// ```ignore
/// #[unique(name = "user_email", comment = "login")]
/// pub email: String,
///
/// #[fulltext(analyzer = "en", bm25, highlights)]
/// pub body: String,
///
/// #[hnsw(dimension = 1536, dist = "cosine", concurrently)]
/// #[diskann(dimension = 1536, type = "f16", name = "post_ann")]
/// pub embedding: Vec<f32>,
/// ```
pub fn parse_field_index_attributes(
    field_name: &str,
    attrs: &[Attribute],
) -> Result<Vec<SpannedIndex>, syn::Error> {
    let mut indexes = Vec::new();
    let mut seen_kinds = BTreeSet::new();

    for attr in attrs {
        let Some(kind_name) = ["unique", "fulltext", "hnsw", "diskann"]
            .into_iter()
            .find(|name| attr.path().is_ident(name))
        else {
            continue;
        };
        if !seen_kinds.insert(kind_name) {
            return Err(syn::Error::new(
                attr.path().span(),
                format!("only one #[{kind_name}] is allowed per field"),
            ));
        }

        let mut modifiers = IndexModifiers::default();
        let kind = match kind_name {
            "unique" => parse_unique_attribute(attr, &mut modifiers)?,
            "fulltext" => parse_fulltext_attribute(attr, &mut modifiers)?,
            "hnsw" => parse_hnsw_attribute(attr, &mut modifiers)?,
            _ => parse_diskann_attribute(attr, &mut modifiers)?,
        };

        indexes.push((
            IndexConfig {
                fields: vec![field_name.to_string()],
                name: modifiers.name,
                kind,
                comment: modifiers.comment,
                concurrently: modifiers.concurrently,
            },
            attr.path().span(),
        ));
    }

    Ok(indexes)
}

/// The position and name of the first entry of `indexes` on `table_name`
/// whose name an earlier entry already uses, if any.
pub fn find_duplicate_index_name(
    table_name: &str,
    indexes: &[IndexConfig],
) -> Option<(usize, String)> {
    let mut seen = BTreeSet::new();
    indexes
        .iter()
        .map(|index| index.index_name(table_name))
        .enumerate()
        .find(|(_, name)| !seen.insert(name.clone()))
}

const INDEXES_EXAMPLE: &str = "#[indexes(\n    user_message(fields(user, message), unique),\n    \
     active_count(count(where = \"active = true\")),\n)]";

/// Parses the struct-level `#[indexes(...)]` attribute into one
/// `IndexConfig` per entry. Each entry is a list named after the index, and
/// every `fields(...)` entry must be rooted at a real struct field
/// (`known_fields` is the snake-cased field name set with any `r#` prefix
/// stripped).
///
/// All struct-level indexes live in one attribute, and every item sits under
/// its (unique) index name, so `clippy::duplicated_attributes` can't fire.
/// Single-field UNIQUE, FULLTEXT, HNSW and DISKANN indexes are declared on
/// the field instead; see [`parse_field_index_attributes`].
///
/// ```ignore
/// #[indexes(
///     user_message(fields(user, message), unique),
///     recent(fields("tags.*", created_at), concurrently),
///     active_count(count(where = "active = true"), comment = "..."),
/// )]
/// ```
pub fn parse_index_attributes(
    attrs: &[Attribute],
    known_fields: &BTreeSet<String>,
) -> Result<Vec<SpannedIndex>, syn::Error> {
    if let Some(old) = attrs.iter().find(|a| a.path().is_ident("index")) {
        return Err(syn::Error::new(
            old.path().span(),
            format!(
                "`#[index(...)]` was replaced by a single `#[indexes(...)]` that names each \
                 index:\n\n{INDEXES_EXAMPLE}"
            ),
        ));
    }

    let mut index_attrs = attrs.iter().filter(|a| a.path().is_ident("indexes"));
    let Some(attr) = index_attrs.next() else {
        return Ok(Vec::new());
    };
    if let Some(extra) = index_attrs.next() {
        return Err(syn::Error::new(
            extra.path().span(),
            "declare all struct-level indexes in a single `#[indexes(...)]`",
        ));
    }

    let mut indexes = Vec::new();
    let mut names = BTreeSet::new();
    attr.parse_nested_meta(|entry| {
        let Some(ident) = entry.path.get_ident() else {
            return Err(entry.error(format!(
                "expected an index name followed by its settings:\n\n{INDEXES_EXAMPLE}"
            )));
        };
        let name = ident.to_string().trim_start_matches("r#").to_string();
        if !names.insert(name.clone()) {
            return Err(entry.error(format!("duplicate index name `{name}`")));
        }
        if !entry.input.peek(syn::token::Paren) {
            return Err(entry.error(format!(
                "expected `{name}(fields(...), ...)`:\n\n{INDEXES_EXAMPLE}"
            )));
        }
        let index = parse_index_entry(&entry, name, known_fields)?;
        indexes.push((index, ident.span()));
        Ok(())
    })?;

    Ok(indexes)
}

/// One `name(...)` entry of `#[indexes(...)]`.
fn parse_index_entry(
    entry: &syn::meta::ParseNestedMeta,
    name: String,
    known_fields: &BTreeSet<String>,
) -> syn::Result<IndexConfig> {
    let mut fields: Option<(Vec<IndexFieldEntry>, proc_macro2::Span)> = None;
    let mut kind: Option<IndexKind> = None;
    let mut modifiers = IndexModifiers::default();

    entry.parse_nested_meta(|meta| {
        if let Some(field_kind) = FIELD_INDEX_ATTRIBUTES
            .iter()
            .find(|k| meta.path.is_ident(k))
        {
            return Err(meta.error(format!(
                "`{field_kind}` indexes cover a single field and are declared on it: \
                 move this to `#[{field_kind}(...)]` on the field"
            )));
        }
        if meta.path.is_ident("name") {
            return Err(meta.error(format!(
                "the entry name `{name}` is the index name; `name = ...` isn't needed here"
            )));
        }
        if (meta.path.is_ident("unique") || meta.path.is_ident("count")) && kind.is_some() {
            return Err(
                meta.error("only one index kind (`unique` or `count`) is allowed per index")
            );
        }
        if modifiers.accept(&meta)? {
            return Ok(());
        }

        if meta.path.is_ident("fields") {
            if fields.is_some() {
                return Err(meta.error("duplicate `fields(...)`"));
            }
            let span = meta.path.span();
            let content;
            parenthesized!(content in meta.input);
            let parsed: Punctuated<IndexFieldEntry, Token![,]> =
                content.parse_terminated(IndexFieldEntry::parse, Token![,])?;
            let collected: Vec<IndexFieldEntry> = parsed.into_iter().collect();
            if collected.is_empty() {
                return Err(
                    meta.error("`fields(...)` must list at least one struct field identifier")
                );
            }
            fields = Some((collected, span));
        } else if meta.path.is_ident("unique") {
            kind = Some(IndexKind::Unique);
        } else if meta.path.is_ident("count") {
            let mut where_clause: Option<String> = None;
            if meta.input.peek(syn::token::Paren) {
                meta.parse_nested_meta(|inner| {
                    if inner.path.is_ident("where") {
                        reject_duplicate(&where_clause, &inner, "where")?;
                        where_clause = Some(index_lit_str(&inner)?.value());
                        Ok(())
                    } else {
                        Err(inner.error("expected `where = \"<condition>\"` inside `count(...)`"))
                    }
                })?;
            }
            kind = Some(IndexKind::Count { where_clause });
        } else {
            return Err(meta.error(INDEX_ATTR_HELP));
        }
        Ok(())
    })?;

    let kind = kind.unwrap_or_default();

    // `#[unique]` on the field covers this; paths such as "tags.*" can't
    // be expressed there, so they stay allowed here.
    if kind == IndexKind::Unique
        && let Some(([IndexFieldEntry::Ident(ident)], span)) = fields
            .as_ref()
            .map(|(entries, span)| (entries.as_slice(), *span))
    {
        return Err(syn::Error::new(
            span,
            format!(
                "a unique index on a single field is declared on the field: \
                 use `#[unique]` on `{}`",
                ident.to_string().trim_start_matches("r#")
            ),
        ));
    }

    let field_names = match (fields, &kind) {
        (Some((_, span)), IndexKind::Count { .. }) => {
            return Err(syn::Error::new(
                span,
                "`count` indexes cannot have `fields(...)`",
            ));
        }
        (None, IndexKind::Count { .. }) => Vec::new(),
        (None, _) => {
            return Err(entry.error(format!(
                "`{name}` needs `fields(<ident>, ...)`, e.g. `{name}(fields(user, message), unique)`"
            )));
        }
        (Some((entries, _)), _) => {
            let mut field_names = Vec::with_capacity(entries.len());
            for field_entry in &entries {
                let (path, root, span) = match field_entry {
                    IndexFieldEntry::Ident(ident) => {
                        let raw = ident.to_string();
                        let field = raw.trim_start_matches("r#").to_string();
                        (field.clone(), field, ident.span())
                    }
                    IndexFieldEntry::Path(lit) => {
                        let path = lit.value().trim().to_string();
                        let root = path
                            .split(['.', '['])
                            .next()
                            .unwrap_or_default()
                            .trim_start_matches("r#")
                            .to_string();
                        (path, root, lit.span())
                    }
                };
                if !known_fields.contains(&root) {
                    let mut all: Vec<&String> = known_fields.iter().collect();
                    all.sort();
                    let listed = all
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(syn::Error::new(
                        span,
                        format!(
                            "unknown field `{}` in #[indexes(...)]; struct has fields: {}",
                            root, listed
                        ),
                    ));
                }
                field_names.push(path);
            }
            field_names
        }
    };

    Ok(IndexConfig {
        fields: field_names,
        name: Some(name),
        kind,
        comment: modifiers.comment,
        concurrently: modifiers.concurrently,
    })
}

pub fn parse_table_validators(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    info!(
        "Starting table validators parsing for {} attributes",
        attrs.len()
    );
    let mut validators = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("validators") {
            debug!("Found validators attribute");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    for meta in metas {
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("custom") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    let validator_value = lit.value();
                                    debug!("Adding custom validator: {}", validator_value);
                                    validators.push(validator_value);
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'custom' parameter must be a string literal containing a validation expression.\n\nExample: #[validators(custom = \"$value > 0 AND $value < 100\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in validators attribute.\n\nValid parameter is: custom\n\nExample: #[validators(custom = \"$value > 0\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in validators attribute.\n\nExpected format: #[validators(custom = \"validation_expression\")]",
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse validators attribute: {}\n\nExample usage:\n#[validators(custom = \"$value > 0\")]\n#[validators(custom = \"string::len($value) > 5\")]",
                            err
                        ),
                    ));
                }
            }
        }
    }

    info!("Successfully parsed {} table validators", validators.len());
    Ok(validators)
}

pub fn parse_relation_attribute(attrs: &[Attribute]) -> Result<Option<EdgeConfig>, syn::Error> {
    info!(
        "Starting relation attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("relation") {
            debug!("Found relation attribute");

            // Handle bare #[relation] with no arguments
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    let mut edge_name = None;
                    let mut direction: Option<Direction> = None;

                    for meta in metas {
                        match meta {
                            Meta::NameValue(nv)
                                if nv.path.is_ident("edge_name") || nv.path.is_ident("name") =>
                            {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    edge_name = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'edge_name' (or 'name') parameter must be a string literal.\n\nExample: #[relation(edge_name = \"has_user\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("direction") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    direction = match lit.value().as_str() {
                                        "from" => Some(Direction::From),
                                        "to" => Some(Direction::To),
                                        "both" => Some(Direction::Both),
                                        other => {
                                            return Err(syn::Error::new(
                                                lit.span(),
                                                format!(
                                                    "Invalid direction '{}'. Valid values are: \"from\", \"to\", \"both\"\n\nExample: direction = \"from\"",
                                                    other
                                                ),
                                            ));
                                        }
                                    };
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'direction' parameter must be a string literal with value \"from\", \"to\", or \"both\".\n\nExample: direction = \"from\"",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in relation attribute.\n\nValid parameters are: edge_name (or name), direction\n\nExamples:\n#[relation]\n#[relation(edge_name = \"custom_name\")]\n#[relation(edge_name = \"custom_name\", direction = \"from\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in relation attribute.\n\nExamples:\n#[relation]\n#[relation(edge_name = \"custom_name\")]\n#[relation(edge_name = \"custom_name\", direction = \"from\")]",
                                ));
                            }
                        }
                    }

                    info!(
                        "Successfully parsed relation attribute: edge_name={:?}, direction={:?}",
                        edge_name, direction
                    );
                    return Ok(Some(EdgeConfig {
                        edge_name: edge_name.unwrap_or_default(),
                        from: vec![],
                        to: vec![],
                        direction,
                    }));
                }
                Err(_) => {
                    // No arguments — bare #[relation]
                    info!("Parsed bare #[relation] attribute");
                    return Ok(Some(EdgeConfig {
                        edge_name: String::new(),
                        from: vec![],
                        to: vec![],
                        direction: None,
                    }));
                }
            }
        }
    }
    debug!("No relation attribute found");
    Ok(None)
}

pub fn parse_doccom_attribute(attrs: &[Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs {
        if attr.path().is_ident("doccom") {
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse doccom attribute: {}\n\nExpected usage: #[doccom(\"Description text\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Doc comment cannot be empty or whitespace-only.\n\nExample: #[doccom(\"A user account in the system\")]",
                ));
            }

            return Ok(Some(value));
        }
    }
    Ok(None)
}

pub fn parse_macroforge_derive_attribute(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    for attr in attrs {
        if attr.path().is_ident("macroforge_derive") {
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    let mut derives = Vec::new();
                    for meta in metas {
                        match meta {
                            Meta::Path(path) => {
                                if let Some(ident) = path.get_ident() {
                                    derives.push(ident.to_string());
                                } else {
                                    return Err(syn::Error::new(
                                        path.span(),
                                        "Expected a simple identifier in macroforge_derive.\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                                    ));
                                }
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Expected bare identifiers in macroforge_derive.\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                                ));
                            }
                        }
                    }
                    return Ok(derives);
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse macroforge_derive attribute: {}\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                            err
                        ),
                    ));
                }
            }
        }
    }
    Ok(Vec::new())
}

/// Extract all derive names from `#[derive(...)]` attributes.
///
/// Returns identifiers like `["Serialize", "Clone", "Debug", "Evenframe"]`.
pub fn parse_rust_derives(attrs: &[Attribute]) -> Vec<String> {
    let mut derives = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("derive")
            && let Meta::List(meta_list) = &attr.meta
        {
            // Parse the token stream as comma-separated paths
            let result: Result<syn::punctuated::Punctuated<syn::Path, syn::Token![,]>, _> =
                meta_list.parse_args_with(syn::punctuated::Punctuated::parse_terminated);
            if let Ok(paths) = result {
                for path in paths {
                    // Use the last segment (e.g., "Serialize" from "serde::Serialize")
                    if let Some(segment) = path.segments.last() {
                        derives.push(segment.ident.to_string());
                    }
                }
            }
        }
    }
    derives
}

pub fn parse_annotation_attributes(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    let mut annotations = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("annotation") {
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse annotation attribute: {}\n\nExpected usage: #[annotation(\"@decorator({{{{ key: \\\"value\\\" }}}})\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Annotation cannot be empty.\n\nExample: #[annotation(\"@default\")]",
                ));
            }

            annotations.push(value);
        }
    }

    Ok(annotations)
}

pub fn parse_serde_enum_representation(
    attrs: &[Attribute],
) -> Result<EnumRepresentation, syn::Error> {
    let mut tag: Option<String> = None;
    let mut content: Option<String> = None;
    let mut untagged = false;

    for attr in attrs {
        if attr.path().is_ident("serde") {
            let nested: syn::punctuated::Punctuated<Meta, syn::Token![,]> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated)?;

            for meta in &nested {
                match meta {
                    Meta::NameValue(nv) if nv.path.is_ident("tag") => {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(lit), ..
                        }) = &nv.value
                        {
                            tag = Some(lit.value());
                        }
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("content") => {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(lit), ..
                        }) = &nv.value
                        {
                            content = Some(lit.value());
                        }
                    }
                    Meta::Path(p) if p.is_ident("untagged") => {
                        untagged = true;
                    }
                    _ => {}
                }
            }
        }
    }

    if untagged {
        return Ok(EnumRepresentation::Untagged);
    }

    match (tag, content) {
        (Some(t), Some(c)) => Ok(EnumRepresentation::AdjacentlyTagged { tag: t, content: c }),
        (Some(t), None) => Ok(EnumRepresentation::InternallyTagged { tag: t }),
        (None, Some(_)) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[serde(content = \"...\")] requires #[serde(tag = \"...\")]",
        )),
        (None, None) => Ok(EnumRepresentation::ExternallyTagged),
    }
}

pub fn parse_format_attribute(
    attrs: &[Attribute],
) -> Result<Option<proc_macro2::TokenStream>, syn::Error> {
    use syn::{Expr, ExprCall, ExprPath, Path, PathSegment};

    info!(
        "Starting format attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("format") {
            debug!("Found format attribute");
            // Parse the attribute content as an expression
            let expr: syn::Expr = attr.parse_args()
                .map_err(|e| syn::Error::new(
                    attr.span(),
                    format!("Failed to parse format attribute: {}\n\nExamples:\n#[format(DateTime)]\n#[format(Url(\"example.com\"))]", e)
                ))?;

            // Transform the expression to add Format:: prefix if needed
            let format_expr = match &expr {
                // If it's just an identifier like DateTime, convert to Format::DateTime
                Expr::Path(path_expr) if path_expr.path.segments.len() == 1 => {
                    let variant = &path_expr.path.segments[0];
                    let mut segments = syn::punctuated::Punctuated::new();
                    segments.push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                    segments.push(variant.clone());
                    Expr::Path(ExprPath {
                        attrs: vec![],
                        qself: None,
                        path: Path {
                            leading_colon: None,
                            segments,
                        },
                    })
                }
                // If it's a call like Url("domain"), convert to Format::Url("domain")
                Expr::Call(call_expr) => {
                    if let Expr::Path(path_expr) = &*call_expr.func {
                        if path_expr.path.segments.len() == 1 {
                            let variant = &path_expr.path.segments[0];
                            let mut segments = syn::punctuated::Punctuated::new();
                            segments
                                .push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                            segments.push(variant.clone());
                            Expr::Call(ExprCall {
                                attrs: call_expr.attrs.clone(),
                                func: Box::new(Expr::Path(ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: Path {
                                        leading_colon: None,
                                        segments,
                                    },
                                })),
                                paren_token: call_expr.paren_token,
                                args: call_expr.args.clone(),
                            })
                        } else {
                            expr.clone()
                        }
                    } else {
                        expr.clone()
                    }
                }
                // Otherwise keep as is
                _ => expr.clone(),
            };

            // Use the TryFrom implementation to parse the Format
            match Format::try_from(&format_expr) {
                Ok(format) => {
                    debug!("Successfully parsed format: {:?}", format);
                    // Since Format implements ToTokens, we can just quote it directly
                    return Ok(Some(quote! { #format }));
                }
                Err(e) => {
                    error!("Failed to parse format expression: {}", e);
                    return Err(syn::Error::new(
                        expr.span(),
                        format!(
                            "{}\n\nValid formats:\n- Simple: DateTime, Date, Time, Currency, Percentage, Phone, Email, FirstName, LastName, CompanyName, PhoneNumber, ColorHex, JwtToken, Oklch, PostalCode\n- With parameter: Url(\"domain.com\")",
                            e
                        ),
                    ));
                }
            }
        }
    }
    debug!("No format attribute found");
    Ok(None)
}

pub fn parse_format_attribute_bin(attrs: &[Attribute]) -> Result<Option<Format>, syn::Error> {
    use syn::{Expr, ExprCall, ExprPath, Path, PathSegment};

    info!(
        "Starting format attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("format") {
            debug!("Found format attribute");
            // Parse the attribute content as an expression
            let expr: syn::Expr = attr.parse_args()
                .map_err(|e| syn::Error::new(
                    attr.span(),
                    format!("Failed to parse format attribute: {}\n\nExamples:\n#[format(DateTime)]\n#[format(Url(\"example.com\"))]", e)
                ))?;

            // Transform the expression to add Format:: prefix if needed
            let format_expr = match &expr {
                // If it's just an identifier like DateTime, convert to Format::DateTime
                Expr::Path(path_expr) if path_expr.path.segments.len() == 1 => {
                    let variant = &path_expr.path.segments[0];
                    let mut segments = syn::punctuated::Punctuated::new();
                    segments.push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                    segments.push(variant.clone());
                    Expr::Path(ExprPath {
                        attrs: vec![],
                        qself: None,
                        path: Path {
                            leading_colon: None,
                            segments,
                        },
                    })
                }
                // If it's a call like Url("domain"), convert to Format::Url("domain")
                Expr::Call(call_expr) => {
                    if let Expr::Path(path_expr) = &*call_expr.func {
                        if path_expr.path.segments.len() == 1 {
                            let variant = &path_expr.path.segments[0];
                            let mut segments = syn::punctuated::Punctuated::new();
                            segments
                                .push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                            segments.push(variant.clone());
                            Expr::Call(ExprCall {
                                attrs: call_expr.attrs.clone(),
                                func: Box::new(Expr::Path(ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: Path {
                                        leading_colon: None,
                                        segments,
                                    },
                                })),
                                paren_token: call_expr.paren_token,
                                args: call_expr.args.clone(),
                            })
                        } else {
                            expr.clone()
                        }
                    } else {
                        expr.clone()
                    }
                }
                // Otherwise keep as is
                _ => expr.clone(),
            };

            // Use the TryFrom implementation to parse the Format
            match Format::try_from(&format_expr) {
                Ok(format) => {
                    debug!("Successfully parsed format: {:?}", format);
                    // Since Format implements ToTokens, we can just quote it directly
                    return Ok(Some(format));
                }
                Err(e) => {
                    error!("Failed to parse format expression: {}", e);
                    return Err(syn::Error::new(
                        expr.span(),
                        format!(
                            "{}\n\nValid formats:\n- Simple: DateTime, Date, Time, Currency, Percentage, Phone, Email, FirstName, LastName, CompanyName, PhoneNumber, ColorHex, JwtToken, Oklch, PostalCode\n- With parameter: Url(\"domain.com\")",
                            e
                        ),
                    ));
                }
            }
        }
    }
    debug!("No format attribute found");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parse_event_attributes_collects_events() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[mock_data(n = 10)]),
            parse_quote!(#[event("DEFINE EVENT foo ON TABLE user WHEN true THEN { RETURN true }")]),
            parse_quote!(#[event("DEFINE EVENT bar ON TABLE user WHEN true THEN { RETURN false }")]),
        ];

        let events = parse_event_attributes(&attrs).expect("expected events to parse");
        assert_eq!(events.len(), 2);
        assert_eq!(
            events,
            vec![
                "DEFINE EVENT foo ON TABLE user WHEN true THEN { RETURN true }".to_string(),
                "DEFINE EVENT bar ON TABLE user WHEN true THEN { RETURN false }".to_string(),
            ]
        );
    }

    #[test]
    fn parse_event_attributes_rejects_empty_statements() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[event("")])];
        let result = parse_event_attributes(&attrs);
        assert!(result.is_err());
    }

    #[test]
    fn parse_serde_no_attrs_returns_externally_tagged() {
        let attrs: Vec<Attribute> = vec![];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(result, EnumRepresentation::ExternallyTagged);
    }

    #[test]
    fn parse_serde_tag_only_returns_internally_tagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(tag = "type")])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::InternallyTagged {
                tag: "type".to_string()
            }
        );
    }

    #[test]
    fn parse_serde_tag_and_content_returns_adjacently_tagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(tag = "t", content = "c")])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::AdjacentlyTagged {
                tag: "t".to_string(),
                content: "c".to_string()
            }
        );
    }

    #[test]
    fn parse_serde_untagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(untagged)])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(result, EnumRepresentation::Untagged);
    }

    #[test]
    fn parse_serde_content_without_tag_errors() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(content = "c")])];
        let result = parse_serde_enum_representation(&attrs);
        assert!(result.is_err());
    }

    #[test]
    fn parse_serde_ignores_non_serde_attrs() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[derive(Debug)]),
            parse_quote!(#[serde(tag = "kind")]),
        ];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::InternallyTagged {
                tag: "kind".to_string()
            }
        );
    }
}
