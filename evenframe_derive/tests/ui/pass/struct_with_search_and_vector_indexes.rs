use evenframe::schemasync::{Bm25, IndexKind, VectorDistance, VectorType};
use evenframe::traits::EvenframePersistableStruct;
use evenframe_derive::Evenframe;

/// Object stored inside `Article`; its paths get struct-level indexes.
#[derive(Debug, Clone, Evenframe)]
pub struct Author {
    pub first_name: String,
    pub bio: String,
    pub embedding: Vec<f32>,
}

/// Struct exercising every index kind and modifier: struct-level composite,
/// count and nested-path indexes, field-level full-text and vector indexes
/// (including two vector indexes on one field and a raw-identifier field).
#[derive(Debug, Clone, Evenframe)]
#[indexes(
    article_tags_created_at(fields("tags.*", created_at), comment = "tags", concurrently),
    article_count(count),
    article_active_count(count(where = "active = true")),
    author_first_name_search(
        fields("author.first_name"),
        fulltext(analyzer = "english", bm25, highlights, comment = "author search"),
        concurrently,
    ),
    author_bio_search(fields("author.bio"), fulltext),
    author_embedding_ann(fields("author.embedding"), hnsw(dimension = 3, dist = "cosine")),
)]
pub struct Article {
    pub id: String,

    #[unique(name = "article_slug", comment = "one per slug", concurrently)]
    pub slug: String,

    #[fulltext(
        name = "article_search",
        analyzer = "english",
        bm25(k1 = 1.2, b = 0.75),
        highlights,
        comment = "full-text search",
        concurrently
    )]
    pub body: String,

    #[fulltext(bm25)]
    pub r#type: String,

    #[fulltext]
    pub title: String,

    pub tags: Vec<String>,
    pub created_at: String,

    #[hnsw(
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
    )]
    #[diskann(
        name = "article_ann",
        dimension = 3,
        dist = "cosine_normalized",
        type = "f16",
        degree = 32,
        l_build = 64,
        alpha = 1,
        hashed_vector
    )]
    pub embedding: Vec<f32>,

    pub active: bool,

    pub author: Author,
}

fn main() {
    let indexes = Article::static_table_config().indexes;
    assert_eq!(indexes.len(), 12);

    // Struct-level indexes come first
    assert_eq!(
        indexes[0].fields,
        vec!["tags.*".to_string(), "created_at".to_string()]
    );
    assert_eq!(indexes[0].name.as_deref(), Some("article_tags_created_at"));
    assert_eq!(indexes[0].comment.as_deref(), Some("tags"));
    assert!(indexes[0].concurrently);
    assert!(indexes[1].fields.is_empty());
    assert_eq!(indexes[1].index_name("article"), "article_count");
    assert_eq!(
        indexes[2].kind,
        IndexKind::Count {
            where_clause: Some("active = true".to_string())
        }
    );

    assert_eq!(indexes[3].fields, vec!["author.first_name".to_string()]);
    assert_eq!(indexes[3].name.as_deref(), Some("author_first_name_search"));
    assert_eq!(indexes[3].comment.as_deref(), Some("author search"));
    assert!(indexes[3].concurrently);
    assert_eq!(
        indexes[3].kind,
        IndexKind::FullText {
            analyzer: Some("english".to_string()),
            bm25: Some(Bm25::Default),
            highlights: true,
        }
    );
    assert_eq!(indexes[4].index_name("article"), "author_bio_search");
    match &indexes[5].kind {
        IndexKind::Hnsw { dimension, dist, .. } => {
            assert_eq!(*dimension, 3);
            assert_eq!(*dist, Some(VectorDistance::Cosine));
        }
        other => panic!("expected hnsw, got {other:?}"),
    }
    assert_eq!(indexes[5].fields, vec!["author.embedding".to_string()]);

    // Then field-level ones, in field order
    let indexes = &indexes[3..];
    assert_eq!(indexes[3].fields, vec!["slug".to_string()]);
    assert_eq!(indexes[3].kind, IndexKind::Unique);
    assert_eq!(indexes[3].name.as_deref(), Some("article_slug"));
    assert_eq!(indexes[3].comment.as_deref(), Some("one per slug"));
    assert!(indexes[3].concurrently);
    assert_eq!(indexes[4].fields, vec!["body".to_string()]);
    assert_eq!(indexes[4].name.as_deref(), Some("article_search"));
    assert_eq!(indexes[4].comment.as_deref(), Some("full-text search"));
    assert!(indexes[4].concurrently);
    assert_eq!(
        indexes[4].kind,
        IndexKind::FullText {
            analyzer: Some("english".to_string()),
            bm25: Some(Bm25::Params { k1: 1.2, b: 0.75 }),
            highlights: true,
        }
    );

    assert_eq!(indexes[5].fields, vec!["type".to_string()]);
    assert_eq!(indexes[5].index_name("article"), "idx_article_type_fulltext");
    assert_eq!(
        indexes[6].kind,
        IndexKind::FullText {
            analyzer: None,
            bm25: None,
            highlights: false,
        }
    );

    match &indexes[7].kind {
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
    assert_eq!(indexes[7].fields, vec!["embedding".to_string()]);
    assert_eq!(indexes[7].index_name("article"), "idx_article_embedding_hnsw");

    match &indexes[8].kind {
        IndexKind::DiskAnn { alpha, dist, .. } => {
            assert_eq!(*alpha, Some(1.0));
            assert_eq!(*dist, Some(VectorDistance::CosineNormalized));
        }
        other => panic!("expected diskann, got {other:?}"),
    }
    assert_eq!(indexes[8].index_name("article"), "article_ann");
}
