//! Runs the evenframe CLI from a copy of the playground against the
//! SurrealDB at `SURREALDB_URL` (`http://localhost:8000` by default), each
//! test in its own namespace, and checks what it leaves in the database.

#[path = "support/binary.rs"]
mod binary;

use binary::{copy_playground, evenframe};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use surrealdb::Surreal;
use surrealdb::engine::remote::http::{Client, Http};
use surrealdb::opt::auth::Root;
use tempfile::TempDir;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

/// A scratch copy of the playground, so tests can change its config and
/// models, with its own namespace, removed again when it is dropped.
struct Project {
    dir: TempDir,
    namespace: String,
}

impl Project {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = TempDir::new().unwrap();
        copy_playground(dir.path());
        let namespace = format!(
            "evenframe_e2e_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        Self { dir, namespace }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// Replaces the single occurrence of `from` in the project file `file`.
    fn edit(&self, file: &str, from: &str, to: &str) {
        let path = self.path(file);
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content.matches(from).count(),
            1,
            "{from} should appear once in {file}"
        );
        fs::write(&path, content.replace(from, to)).unwrap();
    }

    fn run_in(&self, subdir: &str, args: &[&str]) -> Output {
        Command::new(evenframe())
            .args(args)
            .current_dir(self.path(subdir))
            .env(
                "SURREALDB_URL",
                env_or("SURREALDB_URL", "http://localhost:8000"),
            )
            .env("SURREALDB_USER", env_or("SURREALDB_USER", "root"))
            .env("SURREALDB_PASSWORD", env_or("SURREALDB_PASSWORD", "root"))
            .env("SURREALDB_NS", &self.namespace)
            .env("SURREALDB_DB", "playground")
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in("", args)
    }

    fn run_ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "evenframe {args:?} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Runs `sql`, discarding its results.
    fn exec(&self, sql: &str) {
        let namespace = self.namespace.clone();
        let sql = sql.to_string();
        block_on(async move {
            connect(&namespace)
                .await
                .query(sql)
                .await
                .unwrap()
                .check()
                .unwrap();
        });
    }

    /// The value `expression` returns.
    fn value<T: surrealdb::types::SurrealValue + Send + 'static>(&self, expression: &str) -> T {
        let namespace = self.namespace.clone();
        let query = format!("RETURN {expression}");
        block_on(async move {
            let mut response = connect(&namespace).await.query(query).await.unwrap();
            let value: Option<T> = response.take(0).unwrap();
            value.expect("the expression should return a value")
        })
    }

    /// The number of records `select` (a `SELECT … FROM …`) returns.
    fn count(&self, select: &str) -> i64 {
        self.value(&format!("count({select})"))
    }

    /// The items of the list `expression` returns.
    fn values<T: surrealdb::types::SurrealValue + Send + 'static>(
        &self,
        expression: &str,
    ) -> Vec<T> {
        let namespace = self.namespace.clone();
        let query = format!("RETURN {expression}");
        block_on(async move {
            let mut response = connect(&namespace).await.query(query).await.unwrap();
            response.take(0).unwrap()
        })
    }

    /// The record ids of `table`, sorted.
    fn ids(&self, table: &str) -> Vec<String> {
        let mut ids: Vec<String> =
            self.values(&format!("SELECT VALUE type::string(id) FROM {table}"));
        ids.sort();
        ids
    }

    /// `expression` for every record of `table` in id order, as strings.
    fn column(&self, table: &str, expression: &str) -> Vec<String> {
        self.values(&format!(
            "(SELECT VALUE <string> {expression} FROM {table} ORDER BY id)"
        ))
    }

    /// The names under `section` (`accesses`, `analyzers`, `functions`,
    /// `tables`) of `INFO FOR DB`.
    fn defined(&self, section: &str) -> Vec<String> {
        self.values(&format!("object::keys((INFO FOR DB).{section})"))
    }

    fn fields(&self, table: &str) -> Vec<String> {
        self.values(&format!("object::keys((INFO FOR TABLE {table}).fields)"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let namespace = self.namespace.clone();
        block_on(async move {
            let db = connect(&namespace).await;
            if let Err(e) = db
                .query(format!("REMOVE NAMESPACE IF EXISTS {namespace}"))
                .await
                .and_then(|response| response.check())
            {
                eprintln!("failed to remove namespace {namespace}: {e}");
            }
        });
    }
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

async fn connect(namespace: &str) -> Surreal<Client> {
    let url = env_or("SURREALDB_URL", "http://localhost:8000");
    let endpoint = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let db = Surreal::new::<Http>(endpoint)
        .await
        .unwrap_or_else(|e| panic!("SurrealDB should be reachable at {url}: {e}"));
    db.signin(Root {
        username: env_or("SURREALDB_USER", "root"),
        password: env_or("SURREALDB_PASSWORD", "root"),
    })
    .await
    .unwrap();
    db.use_ns(namespace).use_db("playground").await.unwrap();
    db
}

/// `billed_item` links optionally and as a list to `serial_batch_bundle`,
/// which links to `batch_ledger`; neither of those generates records.
fn assert_links_to_empty_tables_stay_empty(project: &Project) {
    assert_eq!(project.count("SELECT id FROM billed_item"), 5);
    assert_eq!(
        project.count(
            "SELECT id FROM billed_item \
             WHERE serial_batch_bundle != NULL AND serial_batch_bundle != NONE"
        ),
        0,
        "optional links to an empty table should be null"
    );
    assert_eq!(
        project.count("SELECT id FROM billed_item WHERE array::len(bundles) > 0"),
        0,
        "list links to an empty table should be empty"
    );
    assert_eq!(project.count("SELECT id FROM serial_batch_bundle"), 0);
    assert_eq!(project.count("SELECT id FROM batch_ledger"), 0);
}

/// Every stored link in the playground points at a record that exists.
/// (`#[edge]` fields are read through their relation table, not stored.)
fn assert_links_point_at_existing_records(project: &Project) {
    assert!(project.count("SELECT id FROM order") > 0);
    for (table, link) in [("purchased", "in"), ("purchased", "out")] {
        assert_eq!(
            project.count(&format!("SELECT id FROM {table} WHERE {link}.id = NONE")),
            0,
            "{table}.{link} should point at existing records"
        );
    }
    assert_eq!(
        project.count(
            "SELECT id FROM billed_item WHERE billable.id = NONE \
             OR record::tb(billable) NOTINSIDE ['product', 'service']"
        ),
        0,
        "union links should point at a product or a service"
    );
}

#[test]
fn schemasync_leaves_links_to_empty_tables_empty() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    assert_links_to_empty_tables_stay_empty(&project);
}

#[test]
fn schemasync_links_point_at_existing_records() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    assert_links_point_at_existing_records(&project);

    // `purchased` coordinates OneToOne on `out`: every order is purchased once.
    let purchases = project.count("SELECT id FROM purchased");
    assert!(purchases > 0);
    assert_eq!(
        project.value::<i64>("count(array::distinct(SELECT VALUE out FROM purchased))"),
        purchases
    );
}

#[test]
fn schemasync_defines_accesses_analyzers_and_functions() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    assert!(project.defined("accesses").contains(&"user".to_string()));
    assert!(
        project
            .defined("analyzers")
            .contains(&"blog_english".to_string())
    );
    assert!(
        project
            .defined("functions")
            .contains(&"billed_item_count".to_string())
    );
    assert_eq!(project.value::<i64>("fn::billed_item_count()"), 5);
}

#[test]
fn schemasync_applies_model_changes_and_keeps_records() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let ids = project.ids("billed_item");
    let billables = project.column("billed_item", "billable");

    project.edit(
        "src/models/billing.rs",
        "pub description: String,",
        "pub note: Option<String>,",
    );
    project.run_ok(&["schemasync"]);

    let fields = project.fields("billed_item");
    assert!(fields.contains(&"note".to_string()), "{fields:?}");
    assert!(!fields.contains(&"description".to_string()), "{fields:?}");
    assert_eq!(
        project.ids("billed_item"),
        ids,
        "existing records should be kept"
    );
    assert_eq!(
        project.column("billed_item", "billable"),
        billables,
        "unchanged fields should keep their values"
    );
    assert_eq!(
        project.count("SELECT id FROM billed_item WHERE description != NONE"),
        0,
        "a removed field should be unset on existing records"
    );
    assert_eq!(
        project.count("SELECT id FROM billed_item WHERE note = NONE"),
        0,
        "a new field should be written to existing records"
    );
}

#[test]
fn schemasync_rewrites_changed_objects_and_keeps_null_ones() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let null_addresses = || project.count("SELECT id FROM customer WHERE shipping_address = NULL");
    let before = null_addresses();

    project.edit(
        "src/models/ecommerce.rs",
        "    pub country: String,\n}",
        "    pub country: String,\n\n    pub unit: String,\n}",
    );
    project.run_ok(&["schemasync"]);

    assert_eq!(
        project.count("SELECT id FROM order WHERE shipping_address.unit = NONE"),
        0,
        "a changed object should be rewritten whole"
    );
    assert_eq!(
        null_addresses(),
        before,
        "an optional object that is NULL should stay NULL"
    );
    assert_eq!(
        project.count(
            "SELECT id FROM customer \
             WHERE shipping_address != NULL AND shipping_address.unit = NONE"
        ),
        0
    );
}

#[test]
fn smart_preservation_rewrites_modified_fields_and_full_keeps_them() {
    for (mode, rewritten) in [("Smart", true), ("Full", false)] {
        let project = Project::new();
        project.edit(
            "evenframe.toml",
            "default_preservation_mode = \"Smart\"",
            &format!("default_preservation_mode = \"{mode}\""),
        );
        project.run_ok(&["schemasync"]);
        let services = project.column("service", "name");

        project.edit(
            "src/models/billing.rs",
            "    pub name: String,\n}",
            "    pub name: Option<String>,\n}",
        );
        project.run_ok(&["schemasync"]);

        assert_eq!(
            project.column("service", "name") != services,
            rewritten,
            "{mode} preservation"
        );
    }
}

#[test]
fn schemasync_removes_excess_records_and_keeps_the_rest() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let ids = project.ids("billed_item");
    let billables = project.column("billed_item", "billable");

    project.edit(
        "src/models/billing.rs",
        "#[mock_data(n = 5)]",
        "#[mock_data(n = 3)]",
    );
    project.run_ok(&["schemasync"]);

    assert_eq!(project.ids("billed_item"), ids[..3]);
    assert_eq!(project.column("billed_item", "billable"), billables[..3]);
}

#[test]
fn schemasync_tops_up_deleted_records_and_full_refresh_resets_them() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);

    project.exec("DELETE billed_item:1");
    project.run_ok(&["schemasync"]);
    assert_eq!(project.ids("billed_item").len(), 5);

    project.exec("DELETE billed_item:2");
    project.run_ok(&["schemasync", "--full-refresh"]);
    assert_eq!(
        project.ids("billed_item"),
        (1..=5)
            .map(|i| format!("billed_item:{i}"))
            .collect::<Vec<_>>()
    );
    assert_links_point_at_existing_records(&project);
}

#[test]
fn schemasync_repoints_links_to_removed_records() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    project.exec("UPDATE billed_item SET billable = service:3");

    project.edit(
        "src/models/billing.rs",
        "#[mock_data(n = 3)]\npub struct Service {",
        "#[mock_data(n = 2)]\npub struct Service {",
    );
    project.run_ok(&["schemasync"]);

    assert_eq!(project.ids("service"), ["service:1", "service:2"]);
    assert_eq!(project.count("SELECT id FROM billed_item"), 5);
    assert_links_point_at_existing_records(&project);
}

#[test]
fn schemasync_repoints_links_nested_in_objects_lists_enums_and_maps() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    project.exec(
        "UPDATE service_booking SET \
         slot = object::extend(slot, { service: service:3 }), \
         slots = array::map(slots, |$s| object::extend($s, { service: service:3 })), \
         kind = { Package: { services: [service:3, service:1] } }, \
         by_day = { monday: service:3 }, \
         backup = { note: 'backup', service: service:3 }; \
         UPDATE service_booking:1 SET backup = NULL",
    );
    let notes = project.column("service_booking", "slot.note");

    project.edit(
        "src/models/billing.rs",
        "#[mock_data(n = 3)]\npub struct Service {",
        "#[mock_data(n = 2)]\npub struct Service {",
    );
    project.run_ok(&["schemasync"]);

    assert_eq!(
        project.count(
            "SELECT id FROM service_booking \
             WHERE <string> [slot, slots, kind, by_day, backup] CONTAINS 'service:3'"
        ),
        0
    );
    assert_eq!(
        project.column("service_booking", "slot.note"),
        notes,
        "the rest of a value should be kept"
    );
    assert_eq!(
        project.value::<String>("<string> service_booking:1.backup"),
        "NULL"
    );
    assert_eq!(
        project.count("SELECT id FROM service_booking WHERE kind.Package.services[1] = service:1"),
        3
    );
}

#[test]
fn schemasync_empties_optional_links_to_tables_that_keep_nothing() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    project.exec(
        "CREATE batch_ledger:1 CONTENT { label: 'ledger' }; \
         CREATE serial_batch_bundle:1 CONTENT { label: 'bundle', ledger: batch_ledger:1 }; \
         UPDATE billed_item SET serial_batch_bundle = serial_batch_bundle:1, \
         bundles = [serial_batch_bundle:1]",
    );

    project.run_ok(&["schemasync"]);

    assert_links_to_empty_tables_stay_empty(&project);
}

#[test]
fn schemasync_rejects_a_required_link_to_a_table_that_keeps_nothing() {
    let project = Project::new();
    project.edit(
        "src/models/billing.rs",
        "#[mock_data(n = 0)]\npub struct SerialBatchBundle",
        "#[mock_data(n = 1)]\npub struct SerialBatchBundle",
    );
    project.run_ok(&["schemasync", "--no-mocks"]);
    project.exec(
        "CREATE batch_ledger:1 CONTENT { label: 'ledger' }; \
         CREATE serial_batch_bundle:1 CONTENT { label: 'bundle', ledger: batch_ledger:1 }",
    );

    let output = project.run(&["schemasync"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("serial_batch_bundle.ledger") && stderr.contains("keeps no records"),
        "{stderr}"
    );
}

#[test]
fn schemasync_without_mocks_keeps_every_record() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    project.edit(
        "src/models/billing.rs",
        "#[mock_data(n = 5)]",
        "#[mock_data(n = 3)]",
    );

    project.run_ok(&["schemasync", "--no-mocks"]);
    assert_eq!(project.count("SELECT id FROM billed_item"), 5);

    project.run_ok(&["schemasync", "--no-mocks", "--full-refresh"]);
    assert_eq!(project.count("SELECT id FROM billed_item"), 5);
    assert!(project.count("SELECT id FROM tag") > 0);
}

#[test]
fn mockmake_repoints_links_to_removed_records() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    project.exec("UPDATE billed_item SET billable = service:3");

    project.run_ok(&["mockmake", "--count", "2", "--tables", "service"]);

    assert_eq!(project.ids("service"), ["service:1", "service:2"]);
    assert_links_point_at_existing_records(&project);
}

#[test]
fn schemasync_applies_every_coordination() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);

    assert_eq!(project.count("SELECT id FROM shift"), 4);
    for (rule, violations) in [
        (
            "equal, into a nested field",
            "location.label != badge_label",
        ),
        (
            "sequential",
            "duration::days(<datetime> ends_on - <datetime> starts_on) != 7",
        ),
        ("sum", "math::abs(deposit + balance - 100) > 0.01"),
        (
            "derive",
            "full_name != string::concat(first_name, ' ', last_name)",
        ),
    ] {
        assert_eq!(
            project.count(&format!("SELECT id FROM shift WHERE {violations}")),
            0,
            "{rule} coordination"
        );
    }
    assert_eq!(
        project.values::<String>("(SELECT VALUE [city, state, zip, country] FROM ONLY shift:1)"),
        ["New York", "NY", "10001", "USA"],
        "coherent coordination"
    );
    // A date that is not on the calendar fails the cast.
    assert!(
        project.count("SELECT id FROM product WHERE <datetime> created_at > d'2000-01-01'") > 0
    );
}

#[test]
fn schemasync_no_mocks_defines_the_schema_only() {
    let project = Project::new();
    project.run_ok(&["schemasync", "--no-mocks"]);
    assert!(
        project
            .defined("tables")
            .contains(&"billed_item".to_string())
    );
    assert_eq!(project.count("SELECT id FROM billed_item"), 0);
    assert_eq!(project.count("SELECT id FROM post"), 0);
}

#[test]
fn mockmake_inserts_around_empty_and_zero_count_tables() {
    let project = Project::new();
    project.run_ok(&["schemasync", "--no-mocks"]);
    project.run_ok(&["mockmake"]);
    assert_links_to_empty_tables_stay_empty(&project);
    assert_links_point_at_existing_records(&project);
}

#[test]
fn mockmake_tables_keeps_other_records_as_link_targets() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let customers = project.ids("customer");

    project.run_ok(&["mockmake", "--tables", "order"]);
    assert_eq!(
        project.ids("customer"),
        customers,
        "unselected tables should keep their records"
    );
    assert_links_point_at_existing_records(&project);
}

#[test]
fn mockmake_regenerates_existing_records_and_keeps_their_ids() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    // Regenerating mostly picks another variant, which must replace the
    // stored one rather than merge into it.
    project.exec("UPDATE service_booking SET kind = { Package: { services: [service:1] } }");
    let ids = project.ids("billed_item");
    let descriptions = project.column("billed_item", "description");
    let null_addresses = || project.count("SELECT id FROM customer WHERE shipping_address = NULL");
    let before = null_addresses();

    project.run_ok(&["mockmake"]);

    assert_eq!(project.ids("billed_item"), ids);
    assert_ne!(project.column("billed_item", "description"), descriptions);
    assert_eq!(
        null_addresses(),
        before,
        "an optional field that is NULL should stay NULL"
    );
    assert_links_to_empty_tables_stay_empty(&project);
    assert_links_point_at_existing_records(&project);
}

#[test]
fn mockmake_keeps_relation_endpoints() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let ids = project.ids("purchased");
    let endpoints = project.column("purchased", "[in, out]");

    project.run_ok(&["mockmake", "--tables", "purchased"]);

    assert_eq!(project.ids("purchased"), ids);
    assert_eq!(project.column("purchased", "[in, out]"), endpoints);
}

#[test]
fn mockmake_count_removes_records_beyond_it() {
    let project = Project::new();
    project.run_ok(&["schemasync"]);
    let tags = project.ids("tag");
    let billed_items = project.ids("billed_item");

    project.run_ok(&["mockmake", "--count", "5", "--tables", "tag"]);

    let mut kept = project.column("tag", "id");
    kept.sort();
    assert_eq!(kept.len(), 5);
    assert!(kept.iter().all(|id| tags.contains(id)), "{kept:?}");
    assert_eq!(
        project.ids("billed_item"),
        billed_items,
        "unselected tables should keep all their records"
    );
}

#[test]
fn mockmake_without_mock_generation_is_an_error() {
    let project = Project::new();
    project.edit(
        "evenframe.toml",
        "should_generate_mocks = true",
        "should_generate_mocks = false",
    );
    project.run_ok(&["schemasync"]);
    let output = project.run(&["mockmake"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("should_generate_mocks is false"),
        "{stderr}"
    );
}

#[test]
fn mockmake_count_overrides_record_counts() {
    let project = Project::new();
    project.run_ok(&["schemasync", "--no-mocks"]);
    project.run_ok(&["mockmake", "--count", "3", "--tables", "service"]);
    assert_eq!(project.count("SELECT id FROM service"), 3);
}

#[test]
fn mockmake_required_link_to_an_empty_table_is_an_error() {
    let project = Project::new();
    project.run_ok(&["schemasync", "--no-mocks"]);
    let output = project.run(&[
        "mockmake",
        "--count",
        "2",
        "--tables",
        "serial_batch_bundle",
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("batch_ledger"), "{stderr}");
}

#[test]
fn mockmake_full_refresh_regenerates_every_table() {
    let project = Project::new();
    project.edit(
        "evenframe.toml",
        "full_refresh_mode = false",
        "full_refresh_mode = true",
    );
    project.run_ok(&["schemasync", "--no-mocks"]);

    let limited = project.run(&["mockmake", "--tables", "billed_item"]);
    let stderr = String::from_utf8_lossy(&limited.stderr);
    assert_eq!(limited.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("full_refresh_mode regenerates every table"),
        "{stderr}"
    );

    project.run_ok(&["mockmake"]);
    assert_links_to_empty_tables_stay_empty(&project);
    assert_links_point_at_existing_records(&project);
    assert_eq!(project.count("SELECT id FROM tag"), 20);
}

#[test]
fn diff_shows_model_changes_and_apply_applies_them() {
    let project = Project::new();
    project.run_ok(&["schemasync", "--no-mocks"]);
    project.edit(
        "src/models/billing.rs",
        "pub description: String,",
        "pub description: String,\n    pub note: Option<String>,",
    );

    let diff = project.run_ok(&["schemasync", "diff", "--format", "json"]);
    assert!(
        diff.contains("billed_item") && diff.contains("note"),
        "{diff}"
    );

    project.run_ok(&["schemasync", "apply", "--yes"]);
    assert!(project.fields("billed_item").contains(&"note".to_string()));
}

#[test]
fn dump_writes_under_the_project_root_from_a_subdirectory() {
    let project = Project::new();
    let output = project.run_in("src", &["schemasync", "dump"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema = fs::read_to_string(project.path(".evenframe/surql/schema.surql")).unwrap();
    assert!(
        schema.contains("DEFINE TABLE OVERWRITE billed_item")
            || schema.contains("DEFINE TABLE billed_item")
    );
}

#[test]
fn generate_runs_typesync_and_schemasync() {
    let project = Project::new();
    project.run_ok(&["generate"]);
    assert!(project.path("src/bindings/arktype.ts").is_file());
    assert!(project.path("src/bindings/schema.proto").is_file());
    assert_eq!(project.count("SELECT id FROM billed_item"), 5);
}
