//! SQL for `access_relations` and `item_access` (T1.1.1). Crate-private: other crates reach these
//! tables only through `access`'s services (ADR-0007), which build on these functions.

use serde_json::Value;
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::model::{ObjectType, Perms, Relation, RelationTuple, SubjectType};

/// One row of `item_access`: the derived permissions of a user on an item.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AccessEntry {
    pub(crate) item_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) perms: Perms,
    /// JSON array explaining the grant ("Why am I seeing this?", V11).
    pub(crate) reasons: Value,
}

fn decode_err(msg: String) -> sqlx::Error {
    sqlx::Error::Decode(msg.into())
}

fn tuple_from_row(
    object_id: Uuid,
    relation: &str,
    subject_id: Uuid,
) -> sqlx::Result<RelationTuple> {
    let relation = Relation::parse(relation)
        .ok_or_else(|| decode_err(format!("unknown relation {relation:?}")))?;
    Ok(RelationTuple::new(object_id, relation, subject_id))
}

fn perms_from_db(bits: i32) -> sqlx::Result<Perms> {
    u8::try_from(bits)
        .ok()
        .and_then(Perms::from_bits)
        .ok_or_else(|| decode_err(format!("invalid perms bitmask {bits}")))
}

/// Inserts a tuple. Returns `false` if it already existed (idempotent grant).
pub(crate) async fn insert_relation(
    conn: impl PgExecutor<'_>,
    tuple: &RelationTuple,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO access_relations (object_type, object_id, relation, subject_type, subject_id)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT DO NOTHING",
    )
    .bind(tuple.object_type().as_str())
    .bind(tuple.object_id())
    .bind(tuple.relation().as_str())
    .bind(tuple.subject_type().as_str())
    .bind(tuple.subject_id())
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Deletes a tuple. Returns `false` if it did not exist.
pub(crate) async fn delete_relation(
    conn: impl PgExecutor<'_>,
    tuple: &RelationTuple,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "DELETE FROM access_relations
         WHERE object_type = $1 AND object_id = $2 AND relation = $3
           AND subject_type = $4 AND subject_id = $5",
    )
    .bind(tuple.object_type().as_str())
    .bind(tuple.object_id())
    .bind(tuple.relation().as_str())
    .bind(tuple.subject_type().as_str())
    .bind(tuple.subject_id())
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// All tuples whose object is `object_type:object_id`.
pub(crate) async fn relations_of_object(
    conn: impl PgExecutor<'_>,
    object_type: ObjectType,
    object_id: Uuid,
) -> sqlx::Result<Vec<RelationTuple>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "SELECT object_id, relation, subject_id FROM access_relations
         WHERE object_type = $1 AND object_id = $2
         ORDER BY relation, subject_type, subject_id",
    )
    .bind(object_type.as_str())
    .bind(object_id)
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|(o, r, s)| tuple_from_row(*o, r, *s))
        .collect()
}

/// Person relations of an item with when each was made (oldest member first, HS2).
pub(crate) async fn user_relations_since(
    conn: impl PgExecutor<'_>,
    item: Uuid,
) -> sqlx::Result<Vec<(RelationTuple, chrono::DateTime<chrono::Utc>)>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Uuid, chrono::DateTime<chrono::Utc>)>(
        "SELECT object_id, relation, subject_id, created_at FROM access_relations
         WHERE object_type = 'item' AND object_id = $1 AND subject_type = 'user'
         ORDER BY created_at, subject_id, relation",
    )
    .bind(item)
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|(o, r, s, at)| tuple_from_row(*o, r, *s).map(|t| (t, *at)))
        .collect()
}

/// All tuples whose subject is `subject_type:subject_id`, optionally limited to one relation
/// (reverse lookups, e.g. pages linked to an item).
pub(crate) async fn relations_of_subject(
    conn: impl PgExecutor<'_>,
    subject_type: SubjectType,
    subject_id: Uuid,
    relation: Option<Relation>,
) -> sqlx::Result<Vec<RelationTuple>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "SELECT object_id, relation, subject_id FROM access_relations
         WHERE subject_type = $1 AND subject_id = $2 AND ($3::text IS NULL OR relation = $3)
         ORDER BY relation, object_type, object_id",
    )
    .bind(subject_type.as_str())
    .bind(subject_id)
    .bind(relation.map(Relation::as_str))
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|(o, r, s)| tuple_from_row(*o, r, *s))
        .collect()
}

fn tuples_from_rows(rows: Vec<(Uuid, String, Uuid)>) -> sqlx::Result<Vec<RelationTuple>> {
    rows.iter()
        .map(|(o, r, s)| tuple_from_row(*o, r, *s))
        .collect()
}

/// All tuples whose object is one of `objects` (forward edges, for `compute_access`).
pub(crate) async fn relations_of_objects(
    conn: impl PgExecutor<'_>,
    objects: &[(ObjectType, Uuid)],
) -> sqlx::Result<Vec<RelationTuple>> {
    let types: Vec<&str> = objects.iter().map(|(t, _)| t.as_str()).collect();
    let ids: Vec<Uuid> = objects.iter().map(|(_, id)| *id).collect();
    let rows = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "SELECT r.object_id, r.relation, r.subject_id
         FROM access_relations r
         JOIN unnest($1::text[], $2::uuid[]) AS o(object_type, object_id)
           ON r.object_type = o.object_type AND r.object_id = o.object_id",
    )
    .bind(&types)
    .bind(&ids)
    .fetch_all(conn)
    .await?;
    tuples_from_rows(rows)
}

/// Tuples through which access flows from one of `subjects` (items or teams) to other items:
/// `linked_to`, `in_notebook` and `owned_by` (reverse edges, for the affected set).
pub(crate) async fn dependent_relations(
    conn: impl PgExecutor<'_>,
    subjects: &[Uuid],
) -> sqlx::Result<Vec<RelationTuple>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "SELECT object_id, relation, subject_id FROM access_relations
         WHERE subject_id = ANY($1) AND relation IN ('linked_to', 'in_notebook', 'owned_by')",
    )
    .bind(subjects)
    .fetch_all(conn)
    .await?;
    tuples_from_rows(rows)
}

/// Replaces the cached access rows of `item_id` with `entries` (entries with no permissions are
/// skipped). Run it inside the transaction that changed the relations (§6.4).
pub(crate) async fn replace_item_access(
    conn: &mut PgConnection,
    item_id: Uuid,
    entries: &[AccessEntry],
) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM item_access WHERE item_id = $1")
        .bind(item_id)
        .execute(&mut *conn)
        .await?;
    let kept: Vec<&AccessEntry> = entries.iter().filter(|e| !e.perms.is_empty()).collect();
    if kept.is_empty() {
        return Ok(());
    }
    let users: Vec<Uuid> = kept.iter().map(|e| e.user_id).collect();
    let perms: Vec<i32> = kept.iter().map(|e| i32::from(e.perms.bits())).collect();
    let reasons: Vec<Value> = kept.iter().map(|e| e.reasons.clone()).collect();
    sqlx::query(
        "INSERT INTO item_access (item_id, user_id, perms, reasons)
         SELECT $1, u, p, r FROM unnest($2::uuid[], $3::int[], $4::jsonb[]) AS t(u, p, r)",
    )
    .bind(item_id)
    .bind(&users)
    .bind(&perms)
    .bind(&reasons)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Narrows the cached access of `item_id` to `entries` without widening it: rows of users not in
/// `entries` are deleted and the others keep only the bits in both (with the new reasons). Used
/// when the widening part of a recompute is deferred (§6.4 step 3).
pub(crate) async fn restrict_item_access(
    conn: &mut PgConnection,
    item_id: Uuid,
    entries: &[AccessEntry],
) -> sqlx::Result<()> {
    let users: Vec<Uuid> = entries.iter().map(|e| e.user_id).collect();
    let perms: Vec<i32> = entries.iter().map(|e| i32::from(e.perms.bits())).collect();
    let reasons: Vec<Value> = entries.iter().map(|e| e.reasons.clone()).collect();
    sqlx::query(
        "WITH n AS (SELECT * FROM unnest($2::uuid[], $3::int[]) AS t(u, p))
         DELETE FROM item_access a
         WHERE a.item_id = $1
           AND NOT EXISTS (SELECT 1 FROM n WHERE n.u = a.user_id AND (n.p & a.perms) <> 0)",
    )
    .bind(item_id)
    .bind(&users)
    .bind(&perms)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE item_access a SET perms = a.perms & n.p, reasons = n.r, updated_at = now()
         FROM unnest($2::uuid[], $3::int[], $4::jsonb[]) AS n(u, p, r)
         WHERE a.item_id = $1 AND a.user_id = n.u",
    )
    .bind(item_id)
    .bind(&users)
    .bind(&perms)
    .bind(&reasons)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Users who can currently view `item_id`, sorted.
pub(crate) async fn viewers(conn: impl PgExecutor<'_>, item_id: Uuid) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT user_id FROM item_access WHERE item_id = $1 AND perms & 1 = 1 ORDER BY user_id",
    )
    .bind(item_id)
    .fetch_all(conn)
    .await
}

/// The cached access of `user_id` on `item_id`, if any.
pub(crate) async fn item_access(
    conn: impl PgExecutor<'_>,
    item_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Option<AccessEntry>> {
    let row = sqlx::query_as::<_, (i32, Value)>(
        "SELECT perms, reasons FROM item_access WHERE item_id = $1 AND user_id = $2",
    )
    .bind(item_id)
    .bind(user_id)
    .fetch_optional(conn)
    .await?;
    row.map(|(bits, reasons)| {
        Ok(AccessEntry {
            item_id,
            user_id,
            perms: perms_from_db(bits)?,
            reasons,
        })
    })
    .transpose()
}

/// Ids of the items on which `user_id` has at least `required`.
pub(crate) async fn accessible_item_ids(
    conn: impl PgExecutor<'_>,
    user_id: Uuid,
    required: Perms,
) -> sqlx::Result<Vec<Uuid>> {
    let required = i32::from(required.bits());
    sqlx::query_scalar(
        "SELECT item_id FROM item_access
         WHERE user_id = $1 AND perms & $2 = $2
         ORDER BY item_id",
    )
    .bind(user_id)
    .bind(required)
    .fetch_all(conn)
    .await
}

#[cfg(test)]
mod tests;
