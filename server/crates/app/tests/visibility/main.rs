//! Visibility rules V1–V12 (PRD §7, ARCHITECTURE §16) as server integration tests against a real
//! Postgres 17. One module per rule; scenarios whose behaviour does not exist yet are `#[ignore]`d
//! with the roadmap task that enables them (T1.1.7). Since T5.1.5 every scenario runs; the
//! checks below keep a newly ignored one tied to a team task.
//!
//! Run the pending ones locally with `cargo test -p app --test visibility -- --ignored`.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test crate: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

mod v01_share_prerequisite;
mod v02_page_viewers;
mod v03_inherited_access;
mod v04_link_permission;
mod v05_member_management;
mod v06_delete_permission;
mod v07_preview_warning;
mod v08_automatic_recompute;
mod v09_hidden_links;
mod v10_access_loss;
mod v11_access_explanation;
mod v12_team_visibility_mode;
mod y2_team_notebooks;

/// Every rule file, keyed by rule id, for the suite's own consistency checks.
const RULE_FILES: [(&str, &str); 12] = [
    ("V1", include_str!("v01_share_prerequisite.rs")),
    ("V2", include_str!("v02_page_viewers.rs")),
    ("V3", include_str!("v03_inherited_access.rs")),
    ("V4", include_str!("v04_link_permission.rs")),
    ("V5", include_str!("v05_member_management.rs")),
    ("V6", include_str!("v06_delete_permission.rs")),
    ("V7", include_str!("v07_preview_warning.rs")),
    ("V8", include_str!("v08_automatic_recompute.rs")),
    ("V9", include_str!("v09_hidden_links.rs")),
    ("V10", include_str!("v10_access_loss.rs")),
    ("V11", include_str!("v11_access_explanation.rs")),
    ("V12", include_str!("v12_team_visibility_mode.rs")),
];

const ROADMAP: &str = include_str!("../../../../../docs/ROADMAP.md");

#[test]
fn every_rule_has_a_file_with_a_scenario() {
    for (rule, source) in RULE_FILES {
        assert!(
            source.starts_with(&format!("//! `{rule}` ")),
            "{rule}: module doc must start with the rule id"
        );
        assert!(
            source.contains("#[tokio::test]"),
            "{rule}: no scenario in the file"
        );
    }
}

#[test]
fn every_ignored_scenario_names_its_roadmap_task() {
    for (rule, source) in RULE_FILES {
        for line in source
            .lines()
            .filter(|l| l.trim_start().starts_with("#[ignore"))
        {
            let task = line
                .trim()
                .strip_prefix("#[ignore = \"")
                .and_then(|rest| rest.split_once(':'))
                .map(|(task, _)| task)
                .unwrap_or_else(|| {
                    panic!("{rule}: `{line}` must be #[ignore = \"T<x.y.z>: <what>\"]")
                });
            assert!(
                task.starts_with('T') && ROADMAP.contains(&format!("**{task}**")),
                "{rule}: `{task}` is not a task in docs/ROADMAP.md"
            );
            // T3.2.6: everything outside teams is on; only team scenarios may still wait.
            assert!(
                task.starts_with("T5.1."),
                "{rule}: `{task}` must be implemented, only team scenarios (T5.1.x) may wait"
            );
        }
    }
}

/// The fixtures run against a fully migrated database that already has the access tables (T1.1.1).
#[tokio::test]
async fn fixtures_use_a_migrated_database_with_access_tables() {
    let world = support::World::new().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = current_schema() AND table_name IN ('access_relations', 'item_access')
         ORDER BY table_name",
    )
    .fetch_all(world.pool())
    .await
    .unwrap();
    assert_eq!(tables, ["access_relations", "item_access"]);
}
