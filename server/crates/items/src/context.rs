//! What `access` needs to know about items (ADR-0007: through an interface, not SQL from
//! `access`).

use std::collections::HashMap;

use access::{AccessContext, ItemAttrs, RolePolicy, TeamVisibility};
use sqlx::PgConnection;
use uuid::Uuid;

/// Item attributes for `compute_access` (team visibility mode, creator; a team page has no
/// creator here, see below). Team role matrices are
/// the defaults until the `teams` module provides them (T5.1.2).
#[derive(Debug, Clone, Copy, Default)]
pub struct ItemsAccessContext;

impl AccessContext for ItemsAccessContext {
    async fn item_attrs(
        &self,
        conn: &mut PgConnection,
        items: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, ItemAttrs>> {
        let rows = sqlx::query_as::<_, (Uuid, Option<String>, Uuid, bool)>(
            "SELECT id, team_visibility, created_by, kind = 'page' AND owner_team_id IS NOT NULL
             FROM items WHERE id = ANY($1)",
        )
        .bind(items)
        .fetch_all(conn)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, mode, created_by, team_page)| {
                let team_visibility = match mode.as_deref() {
                    Some("assignees") => TeamVisibility::Assignees,
                    _ => TeamVisibility::All,
                };
                (
                    id,
                    ItemAttrs {
                        team_visibility,
                        // A team page is seen through its notebook (Y2); on the page itself only
                        // managers and above hold their role (T5.1.5), not whoever wrote it, so
                        // leaving the notebook's chosen people takes its pages away too.
                        created_by: (!team_page).then_some(created_by),
                    },
                )
            })
            .collect())
    }

    async fn team_policies(
        &self,
        _conn: &mut PgConnection,
        _teams: &[Uuid],
    ) -> sqlx::Result<HashMap<Uuid, RolePolicy>> {
        Ok(HashMap::new())
    }
}
