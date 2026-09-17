//! End-to-end tests for index declarations (field-level `#[fulltext]`,
//! `#[hnsw]`, `#[diskann]`; struct-level `#[indexes(...)]` COUNT and
//! array-element composites) and analyzer handling against a real embedded
//! SurrealDB.
//!
//! The snapshot and unit tests only prove what evenframe *emits*. These drive
//! the scanner, apply the generated statements to an in-memory SurrealDB,
//! export the schema back and parse/compare it the same way schemasync does,
//! so they also prove SurrealDB accepts the syntax and that the export parser
//! understands what SurrealDB writes back.

#![cfg(feature = "surrealdb")]

use evenframe_core::schemasync::TableConfig;
use evenframe_core::schemasync::compare::surql::{SchemaImporter, export_schemas};
use evenframe_core::schemasync::compare::{Comparator, SchemaDefinition};
use evenframe_core::schemasync::config::{
    AccessConfig, AccessType, AccessesSource, DatabaseConfig,
};
use evenframe_core::schemasync::database::surql::define::generate_define_statements;
use evenframe_core::schemasync::database::surql::execute::validate_surql_response;
use evenframe_core::schemasync::database::surql::remove::{
    generate_remove_analyzer_statements, generate_remove_index_statements,
};
use evenframe_core::schemasync::dump::{schema_surql, tables_surql};
use evenframe_core::tooling::{BuildConfig, build_all_configs};
use evenframe_core::types::ForeignTypeRegistry;
use std::collections::BTreeMap;
use std::fs;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::engine::remote::http::Client;
use tempfile::TempDir;

const ANALYZERS: &str = "DEFINE ANALYZER OVERWRITE english TOKENIZERS blank, class \
                         FILTERS lowercase, snowball(english);";

const POST_SOURCE: &str = r#"
    #[derive(Evenframe)]
    #[indexes(
        post_tags_created_at(fields("tags.*", created_at)),
        post_count(count),
        post_published_count(count(where = "published = true")),
    )]
    pub struct Post {
        pub id: String,
        #[unique(comment = "one post per slug")]
        pub slug: String,
        #[fulltext(
            name = "post_search",
            analyzer = "english",
            bm25(k1 = 1.2, b = 0.75),
            highlights,
            comment = "full-text search"
        )]
        pub body: String,
        pub tags: Vec<String>,
        pub created_at: String,
        #[hnsw(dimension = 3, dist = "cosine", type = "f32", efc = 100, m = 8)]
        #[diskann(
            name = "post_ann",
            dimension = 3,
            dist = "euclidean",
            type = "f32",
            degree = 16,
            l_build = 50,
            alpha = 1.2
        )]
        pub embedding: Vec<f32>,
        pub published: bool,
    }
"#;

/// Scan a one-file crate containing `source` and return its table configs.
fn scan(source: &str) -> BTreeMap<String, TableConfig> {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"index_e2e_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src/lib.rs"), source).unwrap();

    let config = BuildConfig {
        scan_path: tmp.path().to_path_buf(),
        ..BuildConfig::default()
    };
    let (_enums, tables, _objects) = build_all_configs(&config).expect("build_all_configs");
    tables
}

fn define_statements(tables: &BTreeMap<String, TableConfig>) -> String {
    let registry = ForeignTypeRegistry::default();
    tables
        .iter()
        .map(|(name, table)| {
            generate_define_statements(
                name,
                table,
                tables,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &registry,
                true,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();
    db
}

async fn apply(db: &Surreal<Db>, surql: &str) {
    db.query(surql)
        .await
        .unwrap_or_else(|e| panic!("query failed: {e}\n{surql}"))
        .check()
        .unwrap_or_else(|e| panic!("statement rejected: {e}\n{surql}"));
}

/// Export both databases the way schemasync does and parse the exports.
async fn export_and_parse(
    old: &Surreal<Db>,
    new: &Surreal<Db>,
) -> (SchemaDefinition, SchemaDefinition, String) {
    let (old_export, new_export) = export_schemas(old, new).await.unwrap();
    let client = Surreal::<Client>::init();
    let importer = SchemaImporter::new(&client);
    let old_schema = importer.parse_schema_from_export(&old_export).unwrap();
    let new_schema = importer.parse_schema_from_export(&new_export).unwrap();
    (old_schema, new_schema, new_export)
}

#[tokio::test]
async fn generated_indexes_are_accepted_and_round_trip_through_export() {
    let tables = scan(POST_SOURCE);
    let surql = define_statements(&tables);

    let a = mem_db().await;
    let b = mem_db().await;
    for db in [&a, &b] {
        apply(db, ANALYZERS).await;
        apply(db, &surql).await;
    }

    let (schema_a, schema_b, export) = export_and_parse(&a, &b).await;

    let post = schema_b.tables.get("post").unwrap_or_else(|| {
        panic!("`post` missing from parsed export:\n{export}");
    });
    let index = |name: &str| {
        post.indexes
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "index `{name}` missing; parsed {:?}\n{export}",
                    post.indexes
                )
            })
    };

    let search = index("post_search");
    assert_eq!(search.columns, vec!["body".to_string()]);
    assert!(
        search
            .definition
            .starts_with("FULLTEXT ANALYZER english BM25(1.2,0.75) HIGHLIGHTS"),
        "unexpected fulltext definition: {:?}",
        search.definition
    );
    assert!(
        search.definition.contains("COMMENT"),
        "{:?}",
        search.definition
    );

    assert!(
        index("idx_post_embedding_hnsw")
            .definition
            .starts_with("HNSW DIMENSION 3 DIST COSINE")
    );
    assert!(
        index("post_ann")
            .definition
            .starts_with("DISKANN DIMENSION 3 DIST EUCLIDEAN")
    );
    assert_eq!(
        index("post_tags_created_at").columns,
        vec!["tags.*".to_string(), "created_at".to_string()]
    );
    assert!(index("post_count").columns.is_empty());
    assert_eq!(index("post_count").definition, "COUNT");
    assert!(
        index("post_published_count")
            .definition
            .starts_with("COUNT WHERE")
    );
    assert!(index("idx_post_slug").unique);
    assert_eq!(
        index("idx_post_slug").definition,
        "UNIQUE COMMENT 'one post per slug'",
        "field-level #[unique(...)] options must win over the bare StructField flag"
    );
    assert_eq!(
        post.indexes.iter().filter(|i| i.unique).count(),
        1,
        "#[unique(...)] must not also produce a second unique index"
    );

    assert_eq!(schema_b.analyzers.len(), 1, "{export}");
    assert_eq!(schema_b.analyzers[0].name, "english");

    // Two databases synced from the same source must not report drift.
    let changes = Comparator::compare(&schema_a, &schema_b).unwrap();
    assert!(changes.modified_tables.is_empty(), "{changes:#?}");
    assert!(changes.new_analyzers.is_empty() && changes.modified_analyzers.is_empty());

    // The full-text index is actually usable.
    apply(
        &b,
        "CREATE post:one SET slug = 'one', body = 'Hello search world', tags = ['a'], \
         created_at = '2026-01-01', embedding = [0.1, 0.2, 0.3], published = true;",
    )
    .await;
    let mut response = b
        .query("SELECT VALUE search::score(1) FROM post WHERE body @1@ 'searching';")
        .await
        .unwrap();
    let scores: Vec<f64> = response.take(0).unwrap();
    assert_eq!(
        scores.len(),
        1,
        "stemmed full-text match should find the post"
    );
}

#[tokio::test]
async fn changed_fulltext_parameters_are_detected() {
    let old_tables = scan(
        r#"
        #[derive(Evenframe)]
        pub struct Post {
            pub id: String,
            #[fulltext(name = "post_search", analyzer = "english", bm25)]
            pub body: String,
        }
        "#,
    );
    let new_tables = scan(
        r#"
        #[derive(Evenframe)]
        pub struct Post {
            pub id: String,
            #[fulltext(name = "post_search", analyzer = "english", bm25(k1 = 2.0, b = 0.5), highlights)]
            pub body: String,
        }
        "#,
    );

    let old = mem_db().await;
    let new = mem_db().await;
    apply(&old, ANALYZERS).await;
    apply(&old, &define_statements(&old_tables)).await;
    apply(&new, ANALYZERS).await;
    apply(&new, &define_statements(&new_tables)).await;

    let (old_schema, new_schema, _) = export_and_parse(&old, &new).await;
    let changes = Comparator::compare(&old_schema, &new_schema).unwrap();
    let post = changes
        .modified_tables
        .iter()
        .find(|t| t.table_name == "post")
        .expect("post should be flagged as modified");
    assert_eq!(post.modified_indexes.len(), 1, "{post:#?}");
    assert_eq!(post.modified_indexes[0].name, "post_search");

    // Re-applying the new definition over the old one (what schemasync does
    // for a modified table) must be accepted and converge.
    apply(&old, &define_statements(&new_tables)).await;
    let (old_schema, new_schema, _) = export_and_parse(&old, &new).await;
    let changes = Comparator::compare(&old_schema, &new_schema).unwrap();
    assert!(changes.modified_tables.is_empty(), "{changes:#?}");
}

#[tokio::test]
async fn orphan_index_and_analyzer_are_removed_in_order() {
    let old_tables = scan(
        r#"
        #[derive(Evenframe)]
        pub struct Post {
            pub id: String,
            #[fulltext(name = "post_search", analyzer = "english")]
            pub body: String,
        }
        "#,
    );
    let new_tables = scan(
        r#"
        #[derive(Evenframe)]
        pub struct Post { pub id: String, pub body: String }
        "#,
    );

    let old = mem_db().await;
    let new = mem_db().await;
    apply(&old, ANALYZERS).await;
    apply(&old, &define_statements(&old_tables)).await;
    apply(&new, &define_statements(&new_tables)).await;

    let (old_schema, new_schema, _) = export_and_parse(&old, &new).await;
    let changes = Comparator::compare(&old_schema, &new_schema).unwrap();
    assert_eq!(changes.removed_analyzers, vec!["english".to_string()]);

    let remove_indexes = generate_remove_index_statements(&changes);
    let remove_analyzers = generate_remove_analyzer_statements(&changes);
    assert!(remove_indexes.contains("REMOVE INDEX IF EXISTS post_search ON TABLE post;"));

    // SurrealDB refuses to drop an analyzer that an index still uses, which is
    // why schemasync removes indexes first.
    let premature = old.query(remove_analyzers.as_str()).await.unwrap().check();
    assert!(
        premature.is_err(),
        "removing an analyzer still used by an index should fail"
    );

    apply(&old, &remove_indexes).await;
    apply(&old, &remove_analyzers).await;

    let (old_schema, new_schema, _) = export_and_parse(&old, &new).await;
    let changes = Comparator::compare(&old_schema, &new_schema).unwrap();
    assert!(changes.modified_tables.is_empty(), "{changes:#?}");
    assert!(changes.removed_analyzers.is_empty(), "{changes:#?}");
}

const DOC_WITH_PLAIN_SEARCH: &str = r#"
    #[derive(Evenframe)]
    pub struct Doc {
        pub id: String,
        #[fulltext(name = "doc_search", analyzer = "plain", bm25)]
        pub body: String,
    }
"#;

async fn doc_hits(db: &Surreal<Db>, term: &str) -> usize {
    let mut response = db
        .query(format!("SELECT VALUE id FROM doc WHERE body @@ '{term}';"))
        .await
        .unwrap();
    let ids: Vec<surrealdb::types::RecordId> = response.take(0).unwrap();
    ids.len()
}

/// Sync `live` towards (`analyzers`, `define`) in the same order schemasync
/// uses: compare exports, run removals (orphan indexes before analyzers),
/// apply the analyzers file, then re-apply `DEFINE INDEX` for modified tables.
async fn sync(
    live: &Surreal<Db>,
    analyzers: &str,
    define: &str,
) -> evenframe_core::schemasync::compare::SchemaChanges {
    let desired = mem_db().await;
    apply(&desired, analyzers).await;
    apply(&desired, define).await;

    let (live_schema, desired_schema, _) = export_and_parse(live, &desired).await;
    let changes = Comparator::compare(&live_schema, &desired_schema).unwrap();

    let removals = format!(
        "{}{}",
        generate_remove_index_statements(&changes),
        generate_remove_analyzer_statements(&changes)
    );
    if !removals.trim().is_empty() {
        apply(live, &removals).await;
    }
    apply(live, analyzers).await;
    for table in &changes.modified_tables {
        let on_table = format!(" ON TABLE {} ", table.table_name);
        for stmt in define.split_inclusive(';') {
            let stmt = stmt.trim();
            if stmt.starts_with("DEFINE INDEX") && stmt.contains(&on_table) {
                apply(live, stmt).await;
            }
        }
    }
    changes
}

#[tokio::test]
async fn modified_analyzer_rebuilds_dependent_fulltext_index() {
    let tables = scan(DOC_WITH_PLAIN_SEARCH);
    let define = define_statements(&tables);
    let unstemmed = "DEFINE ANALYZER OVERWRITE plain TOKENIZERS blank FILTERS lowercase;";
    let stemmed =
        "DEFINE ANALYZER OVERWRITE plain TOKENIZERS blank FILTERS lowercase, snowball(english);";

    let live = mem_db().await;
    apply(&live, unstemmed).await;
    apply(&live, &define).await;
    apply(&live, "CREATE doc:one SET body = 'Searching things';").await;
    assert_eq!(doc_hits(&live, "searching").await, 1);
    assert_eq!(doc_hits(&live, "search").await, 0);

    // Overwriting an analyzer does not reindex: on its own it leaves the
    // index returning nothing. This is why dependent indexes get re-applied.
    let bare = mem_db().await;
    apply(&bare, unstemmed).await;
    apply(&bare, &define).await;
    apply(&bare, "CREATE doc:one SET body = 'Searching things';").await;
    apply(&bare, stemmed).await;
    assert_eq!(doc_hits(&bare, "searching").await, 0);

    let changes = sync(&live, stemmed, &define).await;
    assert_eq!(changes.modified_analyzers, vec!["plain".to_string()]);
    let doc = changes
        .modified_tables
        .iter()
        .find(|t| t.table_name == "doc")
        .expect("doc should be revisited for its dependent index");
    assert_eq!(doc.modified_indexes[0].name, "doc_search");

    assert_eq!(doc_hits(&live, "searching").await, 1);
    assert_eq!(
        doc_hits(&live, "search").await,
        1,
        "index rebuilt with stemming"
    );

    let changes = sync(&live, stemmed, &define).await;
    assert!(changes.modified_tables.is_empty(), "{changes:#?}");
    assert!(changes.modified_analyzers.is_empty(), "{changes:#?}");
}

#[tokio::test]
async fn index_can_move_off_an_analyzer_that_is_removed() {
    let old_tables = scan(
        r#"
        #[derive(Evenframe)]
        pub struct Doc {
            pub id: String,
            #[fulltext(name = "doc_search", analyzer = "legacy")]
            pub body: String,
        }
        "#,
    );
    let new_tables = scan(DOC_WITH_PLAIN_SEARCH);
    let new_define = define_statements(&new_tables);
    let new_analyzers =
        "DEFINE ANALYZER OVERWRITE plain TOKENIZERS blank FILTERS lowercase, snowball(english);";

    let live = mem_db().await;
    apply(&live, "DEFINE ANALYZER OVERWRITE legacy TOKENIZERS blank;").await;
    apply(&live, &define_statements(&old_tables)).await;
    apply(&live, "CREATE doc:one SET body = 'Searching things';").await;

    let changes = sync(&live, new_analyzers, &new_define).await;
    assert_eq!(changes.removed_analyzers, vec!["legacy".to_string()]);
    assert_eq!(changes.new_analyzers, vec!["plain".to_string()]);
    assert_eq!(doc_hits(&live, "search").await, 1);

    let changes = sync(&live, new_analyzers, &new_define).await;
    assert!(changes.modified_tables.is_empty(), "{changes:#?}");
    assert!(changes.removed_analyzers.is_empty(), "{changes:#?}");
}

#[tokio::test]
async fn commented_analyzer_file_validates() {
    // Semicolons inside comments and a trailing comment-only fragment must
    // not be counted as statements: SurrealDB returns one result per real
    // statement, and a miscount fails the sync.
    let surql = "-- Analyzers; applied before tables\n\
                 DEFINE ANALYZER OVERWRITE a TOKENIZERS blank; // first; analyzer\n\
                 /* second;\n   analyzer */\n\
                 DEFINE ANALYZER OVERWRITE b\n    TOKENIZERS class\n    FILTERS lowercase;\n\
                 # done;\n";

    let db = mem_db().await;
    let response = db.query(surql).await.unwrap();
    let results = validate_surql_response(response, surql, "define")
        .await
        .unwrap_or_else(|errors| panic!("validation failed: {errors:#?}"));
    assert_eq!(results.len(), 2);

    let (_, schema, _) = export_and_parse(&db, &db).await;
    let names: Vec<&str> = schema.analyzers.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
}

#[tokio::test]
async fn full_schema_dump_applies_top_to_bottom() {
    let tables = scan(POST_SOURCE);
    let tables_surql = tables_surql(
        &tables,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &ForeignTypeRegistry::default(),
        true,
    );

    let mut database = DatabaseConfig::for_testing();
    database.accesses = AccessesSource::Inline(vec![
        AccessConfig {
            name: "reader".to_string(),
            access_type: AccessType::Bearer,
            table_name: "post".to_string(),
        },
        AccessConfig {
            name: "writer".to_string(),
            access_type: AccessType::Bearer,
            table_name: "post".to_string(),
        },
    ]);
    database.resolved.analyzers_surql = Some(format!("-- analyzers; for search\n{ANALYZERS}\n"));
    database.resolved.functions_surql = Some(
        "DEFINE FUNCTION OVERWRITE fn::post_count($p: record<post>) { RETURN count($p) };"
            .to_string(),
    );
    let dump = schema_surql(&database, &tables_surql);

    let db = mem_db().await;
    apply(&db, &dump).await;

    // INFO FOR DB lists each definition as its DEFINE statement
    let mut response = db.query("INFO FOR DB;").await.unwrap();
    let info = response
        .take::<Option<surrealdb::types::Value>>(0)
        .unwrap()
        .map(|v| serde_json::to_string(&v).unwrap())
        .expect("INFO FOR DB result");
    for definition in [
        "DEFINE ACCESS reader ON DATABASE",
        "DEFINE ACCESS writer ON DATABASE",
        "DEFINE ANALYZER english",
        "DEFINE FUNCTION fn::post_count",
        "DEFINE TABLE post",
    ] {
        assert!(info.contains(definition), "missing `{definition}`: {info}");
    }
    let (_, schema, _) = export_and_parse(&db, &db).await;
    assert!(
        schema.tables["post"]
            .indexes
            .iter()
            .any(|i| i.name == "post_search"),
        "full-text index from the dump missing"
    );
}
