use crate::schemasync::mockmake::MockGenerationConfig;
use crate::schemasync::{edge::EdgeConfig, event::EventConfig, permissions::PermissionsConfig};
use crate::types::StructConfig;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableConfig {
    pub table_name: String,
    pub struct_config: StructConfig,
    pub relation: Option<EdgeConfig>,
    pub permissions: Option<PermissionsConfig>,
    pub mock_generation_config: Option<MockGenerationConfig>,
    #[serde(default)]
    pub events: Vec<EventConfig>,
    #[serde(default)]
    pub indexes: Vec<IndexConfig>,
    #[serde(default)]
    pub output_override: Option<Box<TableConfig>>,
}

impl TableConfig {
    /// Resolve `output_override` recursively. Every consumer that reads a
    /// `TableConfig` should call this first — `output_override` is a literal
    /// replacement, applied uniformly across all consumers.
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }

    /// Every index this table declares: one unique index per field-level
    /// `#[unique]`, followed by the struct-level `#[index(...)]` entries.
    /// Entries resolving to the same index name are de-duplicated, with the
    /// struct-level declaration taking precedence.
    /// `table_name` is the name the table is defined under (index names are
    /// derived from it).
    pub fn all_indexes(&self, table_name: &str) -> Vec<IndexConfig> {
        let mut out: Vec<IndexConfig> = self
            .struct_config
            .fields
            .iter()
            .filter(|f| f.unique)
            .map(|f| IndexConfig::unique([f.field_name.clone()]))
            .collect();
        for index in &self.indexes {
            let name = index.index_name(table_name);
            out.retain(|existing| existing.index_name(table_name) != name);
            out.push(index.clone());
        }
        out
    }
}

/// A struct-level index declared via `#[index(...)]` on a
/// `#[derive(Evenframe)]` struct, e.g. `#[index(fields(a, b), unique)]` or
/// `#[index(fields(body), fulltext(analyzer = "en", bm25, highlights))]`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
// The pre-`kind` shape carried `unique: bool`; reject it loudly rather than
// silently deserializing a unique index as a plain one.
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// Indexed field paths. Usually plain field names, but array-element
    /// paths such as `tags.*` are allowed. Empty for `COUNT` indexes.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Explicit index name; defaults to [`IndexConfig::name`]'s derived name.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: IndexKind,
    #[serde(default)]
    pub comment: Option<String>,
    /// Build the index in the background (`CONCURRENTLY`).
    #[serde(default)]
    pub concurrently: bool,
}

/// The kind of a SurrealDB index and its parameters. `None` parameters are
/// left out of the generated statement so SurrealDB applies its defaults.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IndexKind {
    #[default]
    Standard,
    Unique,
    Count {
        #[serde(default)]
        where_clause: Option<String>,
    },
    FullText {
        #[serde(default)]
        analyzer: Option<String>,
        #[serde(default)]
        bm25: Option<Bm25>,
        #[serde(default)]
        highlights: bool,
    },
    Hnsw {
        dimension: u16,
        #[serde(default)]
        dist: Option<VectorDistance>,
        #[serde(default)]
        vector_type: Option<VectorType>,
        #[serde(default)]
        efc: Option<u16>,
        #[serde(default)]
        m: Option<u8>,
        #[serde(default)]
        m0: Option<u8>,
        #[serde(default)]
        lm: Option<f64>,
        #[serde(default)]
        extend_candidates: bool,
        #[serde(default)]
        keep_pruned_connections: bool,
        #[serde(default)]
        hashed_vector: bool,
    },
    DiskAnn {
        dimension: u16,
        #[serde(default)]
        dist: Option<VectorDistance>,
        #[serde(default)]
        vector_type: Option<VectorType>,
        #[serde(default)]
        degree: Option<u32>,
        #[serde(default)]
        l_build: Option<u32>,
        #[serde(default)]
        alpha: Option<f64>,
        #[serde(default)]
        hashed_vector: bool,
    },
}

/// BM25 scoring for a full-text index: `BM25` or `BM25(k1, b)`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bm25 {
    Default,
    Params { k1: f32, b: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorDistance {
    Euclidean,
    Cosine,
    CosineNormalized,
    InnerProduct,
    Manhattan,
    Chebyshev,
    Hamming,
    Jaccard,
    Pearson,
    Minkowski(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorType {
    F64,
    F32,
    F16,
    I64,
    I32,
    I16,
    I8,
    U8,
}

impl VectorDistance {
    pub fn to_surql(&self) -> String {
        match self {
            Self::Euclidean => "EUCLIDEAN".into(),
            Self::Cosine => "COSINE".into(),
            Self::CosineNormalized => "COSINE_NORMALIZED".into(),
            Self::InnerProduct => "INNER_PRODUCT".into(),
            Self::Manhattan => "MANHATTAN".into(),
            Self::Chebyshev => "CHEBYSHEV".into(),
            Self::Hamming => "HAMMING".into(),
            Self::Jaccard => "JACCARD".into(),
            Self::Pearson => "PEARSON".into(),
            Self::Minkowski(order) => format!("MINKOWSKI {order}"),
        }
    }

    /// Whether DISKANN indexes accept this distance.
    pub fn supported_by_diskann(&self) -> bool {
        matches!(
            self,
            Self::Euclidean | Self::Cosine | Self::InnerProduct | Self::CosineNormalized
        )
    }
}

impl std::str::FromStr for VectorDistance {
    type Err = String;

    /// Parses case-insensitively; `minkowski` takes its order, e.g. `"minkowski 3"`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split_whitespace();
        let head = parts.next().unwrap_or_default().to_ascii_lowercase();
        let dist = match head.as_str() {
            "euclidean" => Self::Euclidean,
            "cosine" => Self::Cosine,
            "cosine_normalized" => Self::CosineNormalized,
            "inner_product" => Self::InnerProduct,
            "manhattan" => Self::Manhattan,
            "chebyshev" => Self::Chebyshev,
            "hamming" => Self::Hamming,
            "jaccard" => Self::Jaccard,
            "pearson" => Self::Pearson,
            "minkowski" => {
                let order = parts
                    .next()
                    .and_then(|o| o.parse::<f64>().ok())
                    .ok_or_else(|| {
                        "`minkowski` needs an order, e.g. `dist = \"minkowski 3\"`".to_string()
                    })?;
                Self::Minkowski(order)
            }
            _ => {
                return Err(format!(
                    "unknown distance `{s}`; expected one of euclidean, cosine, \
                     cosine_normalized, inner_product, manhattan, chebyshev, hamming, \
                     jaccard, pearson, minkowski <order>"
                ));
            }
        };
        if parts.next().is_some() {
            return Err(format!("unexpected trailing input in distance `{s}`"));
        }
        Ok(dist)
    }
}

impl VectorType {
    pub fn to_surql(&self) -> &'static str {
        match self {
            Self::F64 => "F64",
            Self::F32 => "F32",
            Self::F16 => "F16",
            Self::I64 => "I64",
            Self::I32 => "I32",
            Self::I16 => "I16",
            Self::I8 => "I8",
            Self::U8 => "U8",
        }
    }

    /// Whether DISKANN indexes accept this vector type.
    pub fn supported_by_diskann(&self) -> bool {
        matches!(self, Self::F32 | Self::F16 | Self::I8 | Self::U8)
    }
}

impl std::str::FromStr for VectorType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "f64" => Self::F64,
            "f32" => Self::F32,
            "f16" => Self::F16,
            "i64" => Self::I64,
            "i32" => Self::I32,
            "i16" => Self::I16,
            "i8" => Self::I8,
            "u8" => Self::U8,
            _ => {
                return Err(format!(
                    "unknown vector type `{s}`; expected one of f64, f32, f16, i64, i32, i16, i8, u8"
                ));
            }
        })
    }
}

impl IndexKind {
    /// Whether this kind indexes exactly one field (FULLTEXT, HNSW, DISKANN).
    pub fn requires_single_field(&self) -> bool {
        matches!(
            self,
            Self::FullText { .. } | Self::Hnsw { .. } | Self::DiskAnn { .. }
        )
    }

    /// The index-kind clause of a `DEFINE INDEX` statement, without a
    /// leading space. Empty for [`IndexKind::Standard`].
    pub fn to_surql(&self) -> String {
        let mut out = String::new();
        match self {
            Self::Standard => {}
            Self::Unique => out.push_str("UNIQUE"),
            Self::Count { where_clause } => {
                out.push_str("COUNT");
                if let Some(cond) = where_clause {
                    out.push_str(" WHERE ");
                    out.push_str(cond);
                }
            }
            Self::FullText {
                analyzer,
                bm25,
                highlights,
            } => {
                out.push_str("FULLTEXT");
                if let Some(az) = analyzer {
                    out.push_str(" ANALYZER ");
                    out.push_str(az);
                }
                match bm25 {
                    None => {}
                    Some(Bm25::Default) => out.push_str(" BM25"),
                    Some(Bm25::Params { k1, b }) => out.push_str(&format!(" BM25({k1},{b})")),
                }
                if *highlights {
                    out.push_str(" HIGHLIGHTS");
                }
            }
            Self::Hnsw {
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
            } => {
                out.push_str(&format!("HNSW DIMENSION {dimension}"));
                if let Some(d) = dist {
                    out.push_str(&format!(" DIST {}", d.to_surql()));
                }
                if let Some(t) = vector_type {
                    out.push_str(&format!(" TYPE {}", t.to_surql()));
                }
                if let Some(v) = efc {
                    out.push_str(&format!(" EFC {v}"));
                }
                if let Some(v) = m {
                    out.push_str(&format!(" M {v}"));
                }
                if let Some(v) = m0 {
                    out.push_str(&format!(" M0 {v}"));
                }
                if let Some(v) = lm {
                    out.push_str(&format!(" LM {v}"));
                }
                if *extend_candidates {
                    out.push_str(" EXTEND_CANDIDATES");
                }
                if *keep_pruned_connections {
                    out.push_str(" KEEP_PRUNED_CONNECTIONS");
                }
                if *hashed_vector {
                    out.push_str(" HASHED_VECTOR");
                }
            }
            Self::DiskAnn {
                dimension,
                dist,
                vector_type,
                degree,
                l_build,
                alpha,
                hashed_vector,
            } => {
                out.push_str(&format!("DISKANN DIMENSION {dimension}"));
                if let Some(d) = dist {
                    out.push_str(&format!(" DIST {}", d.to_surql()));
                }
                if let Some(t) = vector_type {
                    out.push_str(&format!(" TYPE {}", t.to_surql()));
                }
                if let Some(v) = degree {
                    out.push_str(&format!(" DEGREE {v}"));
                }
                if let Some(v) = l_build {
                    out.push_str(&format!(" L_BUILD {v}"));
                }
                if let Some(v) = alpha {
                    out.push_str(&format!(" ALPHA {v}"));
                }
                if *hashed_vector {
                    out.push_str(" HASHED_VECTOR");
                }
            }
        }
        out
    }
}

impl IndexConfig {
    /// A plain (non-unique) index over `fields`.
    pub fn standard<S: Into<String>>(fields: impl IntoIterator<Item = S>) -> Self {
        Self {
            fields: fields.into_iter().map(Into::into).collect(),
            name: None,
            kind: IndexKind::Standard,
            comment: None,
            concurrently: false,
        }
    }

    /// A unique index over `fields`.
    pub fn unique<S: Into<String>>(fields: impl IntoIterator<Item = S>) -> Self {
        Self {
            kind: IndexKind::Unique,
            ..Self::standard(fields)
        }
    }

    pub fn is_unique(&self) -> bool {
        self.kind == IndexKind::Unique
    }

    /// The index name: the explicit `name`, or `idx_{table}_{fields}` with
    /// each field path reduced to identifier characters (`tags.*` → `tags`).
    /// Field-less (COUNT) indexes default to `idx_{table}_count`.
    pub fn index_name(&self, table_name: &str) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        if self.fields.is_empty() {
            return format!("idx_{table_name}_count");
        }
        let joined = self
            .fields
            .iter()
            .map(|f| sanitize_index_name_part(f))
            .collect::<Vec<_>>()
            .join("_");
        format!("idx_{table_name}_{joined}")
    }

    /// Everything after the column list: kind clause, `COMMENT`, `CONCURRENTLY`.
    pub fn definition_clause(&self) -> String {
        let mut parts = Vec::new();
        let kind = self.kind.to_surql();
        if !kind.is_empty() {
            parts.push(kind);
        }
        if let Some(comment) = &self.comment {
            parts.push(format!("COMMENT {}", surql_string_literal(comment)));
        }
        if self.concurrently {
            parts.push("CONCURRENTLY".to_string());
        }
        parts.join(" ")
    }

    /// The full `DEFINE INDEX OVERWRITE ...;` statement for `table_name`.
    pub fn define_statement(&self, table_name: &str) -> String {
        let mut stmt = format!(
            "DEFINE INDEX OVERWRITE {} ON TABLE {}",
            self.index_name(table_name),
            table_name
        );
        if !self.fields.is_empty() {
            stmt.push_str(" FIELDS ");
            stmt.push_str(&self.fields.join(", "));
        }
        let clause = self.definition_clause();
        if !clause.is_empty() {
            stmt.push(' ');
            stmt.push_str(&clause);
        }
        stmt.push(';');
        stmt
    }
}

fn sanitize_index_name_part(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Single-quoted SurrealQL string literal with `\` and `'` escaped.
fn surql_string_literal(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_are_unchanged_for_plain_fields() {
        assert_eq!(
            IndexConfig::unique(["user", "message"]).index_name("reaction"),
            "idx_reaction_user_message"
        );
    }

    #[test]
    fn names_sanitize_paths_and_handle_count() {
        assert_eq!(
            IndexConfig::standard(["tags.*", "age"]).index_name("article"),
            "idx_article_tags_age"
        );
        let count = IndexConfig {
            kind: IndexKind::Count { where_clause: None },
            ..IndexConfig::standard(Vec::<String>::new())
        };
        assert_eq!(count.index_name("post"), "idx_post_count");
        assert_eq!(
            count.define_statement("post"),
            "DEFINE INDEX OVERWRITE idx_post_count ON TABLE post COUNT;"
        );
    }

    #[test]
    fn fulltext_statement() {
        let idx = IndexConfig {
            kind: IndexKind::FullText {
                analyzer: Some("en".into()),
                bm25: Some(Bm25::Params { k1: 1.2, b: 0.75 }),
                highlights: true,
            },
            name: Some("post_search".into()),
            comment: Some("it's searchable".into()),
            concurrently: true,
            ..IndexConfig::standard(["body"])
        };
        assert_eq!(
            idx.define_statement("post"),
            "DEFINE INDEX OVERWRITE post_search ON TABLE post FIELDS body \
             FULLTEXT ANALYZER en BM25(1.2,0.75) HIGHLIGHTS COMMENT 'it\\'s searchable' CONCURRENTLY;"
        );
    }

    #[test]
    fn vector_statements() {
        let hnsw = IndexKind::Hnsw {
            dimension: 3,
            dist: Some(VectorDistance::Minkowski(3.0)),
            vector_type: Some(VectorType::F32),
            efc: Some(150),
            m: Some(12),
            m0: None,
            lm: None,
            extend_candidates: true,
            keep_pruned_connections: false,
            hashed_vector: false,
        };
        assert_eq!(
            hnsw.to_surql(),
            "HNSW DIMENSION 3 DIST MINKOWSKI 3 TYPE F32 EFC 150 M 12 EXTEND_CANDIDATES"
        );
        let diskann = IndexKind::DiskAnn {
            dimension: 4,
            dist: Some(VectorDistance::Cosine),
            vector_type: Some(VectorType::F16),
            degree: None,
            l_build: Some(50),
            alpha: Some(1.5),
            hashed_vector: true,
        };
        assert_eq!(
            diskann.to_surql(),
            "DISKANN DIMENSION 4 DIST COSINE TYPE F16 L_BUILD 50 ALPHA 1.5 HASHED_VECTOR"
        );
    }

    #[test]
    fn parses_distances_and_types() {
        assert_eq!(
            "Cosine_Normalized".parse::<VectorDistance>(),
            Ok(VectorDistance::CosineNormalized)
        );
        assert_eq!(
            "minkowski 2.5".parse::<VectorDistance>(),
            Ok(VectorDistance::Minkowski(2.5))
        );
        assert!("minkowski".parse::<VectorDistance>().is_err());
        assert!("cosine x".parse::<VectorDistance>().is_err());
        assert_eq!("I8".parse::<VectorType>(), Ok(VectorType::I8));
        assert!("f8".parse::<VectorType>().is_err());
    }

    #[test]
    fn legacy_shape_without_kind_deserializes() {
        let idx: IndexConfig = serde_json::from_str(r#"{"fields": ["a"]}"#).unwrap();
        assert_eq!(idx, IndexConfig::standard(["a"]));
        let idx: IndexConfig =
            serde_json::from_str(r#"{"fields": ["a", "b"], "kind": {"type": "unique"}}"#).unwrap();
        assert!(idx.is_unique());
        assert!(
            serde_json::from_str::<IndexConfig>(r#"{"fields": ["a"], "unique": true}"#).is_err(),
            "legacy `unique` flag must not be silently ignored"
        );
    }
}
