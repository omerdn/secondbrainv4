//! `access`: Relation tuples, access computation and `PermissionService` (ADR-0004).
//!
//! Owns the `access_relations` and `item_access` tables (ARCHITECTURE §5.11, §6). Other crates
//! never touch them with SQL (ADR-0007).

mod model;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used by the access engine and PermissionService (T1.1.3, T1.1.5)"
    )
)]
mod repo;

pub use model::{ObjectType, Perms, Relation, RelationTuple, SubjectType};
