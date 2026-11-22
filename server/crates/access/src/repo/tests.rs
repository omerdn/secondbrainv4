//! Repository integration tests against a real Postgres 17 (T1.1.1). Requires Docker (or
//! `TEST_DATABASE_URL`).

// Test module: panics are the failure mechanism.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use db::testing::TestDb;
use serde_json::json;

use super::*;
use crate::model::{ObjectType, Perms, Relation, RelationTuple, SubjectType};

fn id() -> Uuid {
    Uuid::now_v7()
}

/// Raw insert bypassing the Rust types, to exercise the database constraints.
async fn raw_relation(
    db: &TestDb,
    object_type: &str,
    relation: &str,
    subject_type: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO access_relations (object_type, object_id, relation, subject_type, subject_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(object_type)
    .bind(id())
    .bind(relation)
    .bind(subject_type)
    .bind(id())
    .execute(db.pool())
    .await
    .map(|_| ())
}

fn violated(result: Result<(), sqlx::Error>) -> String {
    let err = result.expect_err("statement should violate a constraint");
    err.as_database_error()
        .and_then(|e| e.constraint())
        .unwrap_or_else(|| panic!("not a constraint violation: {err}"))
        .to_owned()
}

#[tokio::test]
async fn every_rust_relation_shape_is_accepted_and_others_rejected() {
    let db = TestDb::new().await;
    for r in Relation::ALL {
        for o in ObjectType::ALL {
            for s in SubjectType::ALL {
                let res = raw_relation(&db, o.as_str(), r.as_str(), s.as_str()).await;
                if *o == r.object_type() && *s == r.subject_type() {
                    res.unwrap_or_else(|e| panic!("{o} {r} {s} rejected: {e}"));
                } else {
                    assert_eq!(violated(res), "access_relations_shape_check", "{o} {r} {s}");
                }
            }
        }
    }
    assert_eq!(
        violated(raw_relation(&db, "item", "god", "user").await),
        "access_relations_shape_check"
    );
    assert_eq!(
        violated(raw_relation(&db, "page", "owner", "user").await),
        "access_relations_object_type_check"
    );
    // Postgres evaluates CHECKs in name order, so the shape check may fire first here.
    let bad_subject = violated(raw_relation(&db, "item", "owner", "robot").await);
    assert!(
        [
            "access_relations_subject_type_check",
            "access_relations_shape_check"
        ]
        .contains(&bad_subject.as_str()),
        "{bad_subject}"
    );
}

#[tokio::test]
async fn insert_is_idempotent_and_delete_reports_presence() {
    let db = TestDb::new().await;
    let t = RelationTuple::new(id(), Relation::Owner, id());
    assert!(insert_relation(db.pool(), &t).await.unwrap());
    assert!(!insert_relation(db.pool(), &t).await.unwrap());
    assert_eq!(
        relations_of_object(db.pool(), ObjectType::Item, t.object_id())
            .await
            .unwrap(),
        vec![t]
    );
    assert!(delete_relation(db.pool(), &t).await.unwrap());
    assert!(!delete_relation(db.pool(), &t).await.unwrap());
    assert!(
        relations_of_object(db.pool(), ObjectType::Item, t.object_id())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn lookups_by_object_and_subject() {
    let db = TestDb::new().await;
    let (task, page_a, page_b, notebook, team) = (id(), id(), id(), id(), id());
    let (ali, ayse) = (id(), id());
    let tuples = [
        RelationTuple::new(task, Relation::Owner, ali),
        RelationTuple::new(task, Relation::Assignee, ayse),
        RelationTuple::new(page_a, Relation::LinkedTo, task),
        RelationTuple::new(page_b, Relation::LinkedTo, task),
        RelationTuple::new(page_a, Relation::InNotebook, notebook),
        RelationTuple::new(team, Relation::TeamManager, ayse),
    ];
    for t in &tuples {
        insert_relation(db.pool(), t).await.unwrap();
    }

    let of_task = relations_of_object(db.pool(), ObjectType::Item, task)
        .await
        .unwrap();
    assert_eq!(of_task, vec![tuples[1], tuples[0]]); // ordered by relation name

    // The same uuid as a team object has no item tuples and vice versa.
    assert!(
        relations_of_object(db.pool(), ObjectType::Team, task)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        relations_of_object(db.pool(), ObjectType::Team, team)
            .await
            .unwrap(),
        vec![tuples[5]]
    );

    // Pages linked to the task (reverse lookup).
    let mut linked: Vec<Uuid> =
        relations_of_subject(db.pool(), SubjectType::Item, task, Some(Relation::LinkedTo))
            .await
            .unwrap()
            .iter()
            .map(RelationTuple::object_id)
            .collect();
    linked.sort();
    let mut expected = vec![page_a, page_b];
    expected.sort();
    assert_eq!(linked, expected);

    // Everything pointing at ayse, across object types.
    let of_ayse = relations_of_subject(db.pool(), SubjectType::User, ayse, None)
        .await
        .unwrap();
    assert_eq!(of_ayse, vec![tuples[1], tuples[5]]);

    // Filter by a relation she does not have.
    assert!(
        relations_of_subject(db.pool(), SubjectType::User, ayse, Some(Relation::Owner))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn unknown_relation_in_db_is_a_decode_error() {
    let db = TestDb::new().await;
    let object = id();
    // Simulate a relation added by a newer migration that this binary does not know yet.
    let mut tx = db.pool().begin().await.unwrap();
    sqlx::query("ALTER TABLE access_relations DROP CONSTRAINT access_relations_shape_check")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO access_relations (object_type, object_id, relation, subject_type, subject_id)
         VALUES ('item', $1, 'future', 'user', $2)",
    )
    .bind(object)
    .bind(id())
    .execute(&mut *tx)
    .await
    .unwrap();
    let err = relations_of_object(&mut *tx, ObjectType::Item, object)
        .await
        .unwrap_err();
    assert!(matches!(err, sqlx::Error::Decode(_)), "{err}");
}

#[tokio::test]
async fn item_access_replace_read_and_list() {
    let db = TestDb::new().await;
    let (item, other_item, ali, ayse, can) = (id(), id(), id(), id(), id());

    let mut conn = db.pool().acquire().await.unwrap();
    replace_item_access(
        &mut conn,
        item,
        &[
            AccessEntry {
                item_id: item,
                user_id: ali,
                perms: Perms::ALL,
                reasons: json!([{ "kind": "owner" }]),
            },
            AccessEntry {
                item_id: item,
                user_id: ayse,
                perms: Perms::VIEW | Perms::EDIT,
                reasons: json!([{ "kind": "assignee" }]),
            },
            // No permissions: not stored.
            AccessEntry {
                item_id: item,
                user_id: can,
                perms: Perms::NONE,
                reasons: json!([]),
            },
        ],
    )
    .await
    .unwrap();
    replace_item_access(
        &mut conn,
        other_item,
        &[AccessEntry {
            item_id: other_item,
            user_id: ayse,
            perms: Perms::VIEW,
            reasons: json!([{ "kind": "watcher" }]),
        }],
    )
    .await
    .unwrap();

    let ayse_on_item = item_access(&mut *conn, item, ayse).await.unwrap().unwrap();
    assert_eq!(ayse_on_item.perms, Perms::VIEW | Perms::EDIT);
    assert_eq!(ayse_on_item.reasons, json!([{ "kind": "assignee" }]));
    assert!(item_access(&mut *conn, item, can).await.unwrap().is_none());

    let mut both = vec![item, other_item];
    both.sort();
    assert_eq!(
        accessible_item_ids(&mut *conn, ayse, Perms::VIEW)
            .await
            .unwrap(),
        both
    );
    assert_eq!(
        accessible_item_ids(&mut *conn, ayse, Perms::EDIT)
            .await
            .unwrap(),
        vec![item]
    );
    assert!(
        accessible_item_ids(&mut *conn, ayse, Perms::VIEW | Perms::DELETE)
            .await
            .unwrap()
            .is_empty()
    );

    // Replacing drops users who are no longer listed and leaves other items alone.
    replace_item_access(
        &mut conn,
        item,
        &[AccessEntry {
            item_id: item,
            user_id: ali,
            perms: Perms::ALL,
            reasons: json!([{ "kind": "owner" }]),
        }],
    )
    .await
    .unwrap();
    assert!(item_access(&mut *conn, item, ayse).await.unwrap().is_none());
    assert_eq!(
        accessible_item_ids(&mut *conn, ayse, Perms::VIEW)
            .await
            .unwrap(),
        vec![other_item]
    );

    // Replacing with nothing clears the item.
    replace_item_access(&mut conn, item, &[]).await.unwrap();
    assert!(item_access(&mut *conn, item, ali).await.unwrap().is_none());
}

#[tokio::test]
async fn item_access_constraints() {
    let db = TestDb::new().await;
    let insert = |perms: i32, reasons: Value| {
        let pool = db.pool().clone();
        async move {
            sqlx::query(
                "INSERT INTO item_access (item_id, user_id, perms, reasons) VALUES ($1, $2, $3, $4)",
            )
            .bind(id())
            .bind(id())
            .bind(perms)
            .bind(reasons)
            .execute(&pool)
            .await
            .map(|_| ())
        }
    };
    insert(63, json!([])).await.unwrap();
    assert_eq!(
        violated(insert(0, json!([])).await),
        "item_access_perms_check"
    );
    assert_eq!(
        violated(insert(64, json!([])).await),
        "item_access_perms_check"
    );
    assert_eq!(
        violated(insert(1, json!({})).await),
        "item_access_reasons_check"
    );
}

#[tokio::test]
async fn lookup_indexes_exist() {
    let db = TestDb::new().await;
    let defs: Vec<String> = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes
         WHERE schemaname = current_schema() AND tablename IN ('access_relations', 'item_access')
         ORDER BY indexname",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    let has = |needle: &str| defs.iter().any(|d| d.contains(needle));
    // Primary keys cover lookups by object and by (item, user).
    assert!(
        has("(object_type, object_id, relation, subject_type, subject_id)"),
        "{defs:?}"
    );
    assert!(has("item_access_pkey ON"), "{defs:?}");
    // Reverse lookups by subject and "items this user can access".
    assert!(has("(subject_type, subject_id, relation)"), "{defs:?}");
    assert!(has("item_access_user_idx ON"), "{defs:?}");
    assert!(has("(user_id, item_id)"), "{defs:?}");
}
