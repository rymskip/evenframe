use evenframe::schemasync::{Bm25, IndexKind, VectorDistance, VectorType};
use evenframe::traits::EvenframePersistableStruct;
use evenframe_derive::Evenframe;

/// Struct exercising every `#[index(...)]` kind and modifier, including
/// string field paths and a raw-identifier field.
#[derive(Debug, Clone, Evenframe)]
#[index(
    name = "article_search",
    fields(body),
    fulltext(analyzer = "english", bm25(k1 = 1.2, b = 0.75), highlights),
    comment = "full-text search",
    concurrently
)]
#[index(fields(r#type), fulltext(bm25))]
#[index(fields(embedding), hnsw(
    dimension = 3,
    dist = "minkowski 3",
    type = "F32",
    efc = 150,
    m = 12,
    m0 = 24,
    lm = 0.4,
    extend_candidates,
    keep_pruned_connections,
    hashed_vector
))]
#[index(
    name = "article_ann",
    fields(embedding),
    diskann(dimension = 3, dist = "cosine_normalized", type = "f16", degree = 32, l_build = 64, alpha = 1, hashed_vector)
)]
#[index(fields("tags.*", created_at))]
#[index(count)]
#[index(name = "article_active_count", count(where = "active = true"))]
pub struct Article {
    pub id: String,
    pub body: String,
    pub r#type: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub embedding: Vec<f32>,
    pub active: bool,
}

fn main() {
    let indexes = Article::static_table_config().indexes;
    assert_eq!(indexes.len(), 7);

    assert_eq!(indexes[0].name.as_deref(), Some("article_search"));
    assert_eq!(indexes[0].comment.as_deref(), Some("full-text search"));
    assert!(indexes[0].concurrently);
    assert_eq!(
        indexes[0].kind,
        IndexKind::FullText {
            analyzer: Some("english".to_string()),
            bm25: Some(Bm25::Params { k1: 1.2, b: 0.75 }),
            highlights: true,
        }
    );

    assert_eq!(indexes[1].fields, vec!["type".to_string()]);

    match &indexes[2].kind {
        IndexKind::Hnsw {
            dimension,
            dist,
            vector_type,
            m0,
            lm,
            hashed_vector,
            ..
        } => {
            assert_eq!(*dimension, 3);
            assert_eq!(*dist, Some(VectorDistance::Minkowski(3.0)));
            assert_eq!(*vector_type, Some(VectorType::F32));
            assert_eq!(*m0, Some(24));
            assert_eq!(*lm, Some(0.4));
            assert!(*hashed_vector);
        }
        other => panic!("expected hnsw, got {other:?}"),
    }

    match &indexes[3].kind {
        IndexKind::DiskAnn { alpha, dist, .. } => {
            assert_eq!(*alpha, Some(1.0));
            assert_eq!(*dist, Some(VectorDistance::CosineNormalized));
        }
        other => panic!("expected diskann, got {other:?}"),
    }

    assert_eq!(
        indexes[4].fields,
        vec!["tags.*".to_string(), "created_at".to_string()]
    );
    assert!(indexes[5].fields.is_empty());
    assert_eq!(
        indexes[6].kind,
        IndexKind::Count {
            where_clause: Some("active = true".to_string())
        }
    );
}
