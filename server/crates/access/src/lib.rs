//! `access`: Relation tuples, access computation and `PermissionService` (ADR-0004).
//!
//! Owns the `access_relations` and `item_access` tables (ARCHITECTURE §5.11, §6). Other crates
//! never touch them with SQL (ADR-0007).

mod affected;
mod compute;
mod model;
mod recompute;
mod repo;

pub use affected::{Change, affected_items};
pub use compute::{
    Access, AccessGraph, ItemAttrs, Reason, RolePolicy, TeamRole, TeamVisibility, compute_access,
};
pub use model::{ObjectType, Perms, Relation, RelationTuple, SubjectType};
pub use recompute::{
    AccessConfig, AccessContext, AccessEngine, DefaultAccessContext, ExpandAccess, Recomputed,
};
