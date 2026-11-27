//! Team items (PRD V12, TK4, N3, V8, T5.1.3): items owned by a team instead of a person.
//!
//! - A team item is created by an active member whose role may create items (TK3). Its
//!   visibility mode is chosen at creation (V12): the whole team, or only the creator, the
//!   assignees, the watchers and managers and above. `access` computes who sees it.
//! - Assignees and watchers are chosen separately (TK4); the creator is not made an assignee
//!   (N3). Team assignments need no answer (TK2) and are written directly as relations. Choosing
//!   someone else needs the role's "assign" right; assigning oneself does not.
//! - Changing the mode needs MANAGE on the item and recomputes access at once (V8).
//!
//! `items` does not read the `teams` tables (ADR-0007): it asks a [`TeamDirectory`], which the
//! `teams` module implements.

use std::future::Future;
use std::pin::Pin;

use access::{Change, Perms, Relation, RelationTuple};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::activity::{self, ActivityKind, NewActivity};
use crate::invitations::{InvitationRole, role_allowed};
use crate::model::{CreateItemRequest, Item, NewItem};
use crate::service::{COLUMNS, ItemRow, ItemsService};

/// Most assignees plus watchers one team item is created with.
pub const TEAM_ITEM_MAX_PEOPLE: usize = 50;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn invalid(field: &str, message: &str) -> AppError {
    ValidationErrors::from(FieldError::new(field, FieldErrorCode::Invalid, message)).into()
}

/// Who of the team sees a team item (V12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TeamVisibilityMode {
    /// Every team member, according to their role.
    #[default]
    All,
    /// Only the creator, the assignees, the watchers and managers and above.
    Assignees,
}

impl TeamVisibilityMode {
    /// The stored name (`items.team_visibility`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Assignees => "assignees",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "assignees" => Some(Self::Assignees),
            _ => None,
        }
    }
}

/// What a team member does with team items, checked against the role matrix (TK3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamAction {
    CreateItems,
    /// Make someone else an assignee or watcher.
    Assign,
}

/// Result of a [`TeamDirectory`] check.
pub type TeamFuture<'a> = Pin<Box<dyn Future<Output = Result<bool, AppError>> + Send + 'a>>;

/// What `items` needs to know about teams; implemented by the `teams` module.
pub trait TeamDirectory: Send + Sync {
    /// Whether `user` is an active member of `team`.
    fn is_member(&self, team: Uuid, user: Uuid) -> TeamFuture<'_>;
    /// Whether `actor` is an active member whose role allows `action` in `team`.
    fn may(&self, actor: Uuid, team: Uuid, action: TeamAction) -> TeamFuture<'_>;
}

/// Knows no team. The default until the `teams` module is wired in.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTeams;

impl TeamDirectory for NoTeams {
    fn is_member(&self, _team: Uuid, _user: Uuid) -> TeamFuture<'_> {
        Box::pin(async { Ok(false) })
    }
    fn may(&self, _actor: Uuid, _team: Uuid, _action: TeamAction) -> TeamFuture<'_> {
        Box::pin(async { Ok(false) })
    }
}

/// `POST /teams/{team_id}/items` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateTeamItemRequest {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub description: Option<String>,
    /// `all` when absent (V12).
    #[serde(default)]
    #[ts(optional)]
    pub visibility: Option<TeamVisibilityMode>,
    /// Team members responsible for the item (TK4); the creator is not added (N3).
    #[serde(default)]
    #[ts(optional)]
    pub assignees: Option<Vec<Uuid>>,
    #[serde(default)]
    #[ts(optional)]
    pub watchers: Option<Vec<Uuid>>,
}

/// A validated new team item: no person twice (an assignee is not also a watcher).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTeamItem {
    pub item: NewItem,
    pub visibility: TeamVisibilityMode,
    pub assignees: Vec<Uuid>,
    pub watchers: Vec<Uuid>,
}

/// `PUT /items/{id}/visibility` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct SetTeamVisibilityRequest {
    pub visibility: TeamVisibilityMode,
}

fn unique(people: Option<Vec<Uuid>>, skip: &[Uuid]) -> Vec<Uuid> {
    let mut out: Vec<Uuid> = Vec::new();
    for person in people.unwrap_or_default() {
        if !skip.contains(&person) && !out.contains(&person) {
            out.push(person);
        }
    }
    out
}

impl CreateTeamItemRequest {
    /// # Errors
    /// [`AppError::Validation`] for an invalid item, a kind teams cannot own yet (notebooks and
    /// pages come with team notebooks, T5.1.5), people the kind has no role for, or too many
    /// people.
    pub fn validate(self) -> Result<NewTeamItem, AppError> {
        if matches!(self.kind.as_str(), "notebook" | "page") {
            return Err(invalid("kind", "teams cannot own this item kind yet"));
        }
        let item = CreateItemRequest {
            kind: self.kind,
            title: self.title,
            description: self.description,
        }
        .validate()?;
        let assignees = unique(self.assignees, &[]);
        let watchers = unique(self.watchers, &assignees);
        let mut errors = ValidationErrors::new();
        for (field, people, role) in [
            ("assignees", &assignees, InvitationRole::Assignee),
            ("watchers", &watchers, InvitationRole::Watcher),
        ] {
            if !people.is_empty() && !role_allowed(&item.kind, role) {
                errors.push(FieldError::new(
                    field,
                    FieldErrorCode::Invalid,
                    "this item kind has no such role",
                ));
            }
        }
        if assignees.len() + watchers.len() > TEAM_ITEM_MAX_PEOPLE {
            errors.push(FieldError::new(
                "assignees",
                FieldErrorCode::Invalid,
                "too many people",
            ));
        }
        errors.into_result()?;
        Ok(NewTeamItem {
            item,
            visibility: self.visibility.unwrap_or_default(),
            assignees,
            watchers,
        })
    }
}

impl ItemsService {
    /// Creates an item owned by `team` (V12, TK4).
    ///
    /// # Errors
    /// See [`Self::create_team_item_in`].
    pub async fn create_team_item(
        &self,
        actor: Uuid,
        team: Uuid,
        new: NewTeamItem,
    ) -> Result<Item, AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.create_team_item_in(&mut tx, actor, team, new).await?;
        tx.commit().await.map_err(internal)?;
        Ok(item)
    }

    /// [`Self::create_team_item`] inside the caller's transaction, for modules whose detail row
    /// must exist together with the item. Ownership, assignees and watchers are written in it.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the actor is not an active member of,
    /// [`AppError::Forbidden`] when the actor's role may not create items or assign others,
    /// [`AppError::Validation`] for a person who is not an active member, or
    /// [`AppError::Internal`].
    pub async fn create_team_item_in(
        &self,
        conn: &mut sqlx::PgConnection,
        actor: Uuid,
        team: Uuid,
        new: NewTeamItem,
    ) -> Result<Item, AppError> {
        if !self.teams.is_member(team, actor).await? {
            return Err(AppError::NotFound);
        }
        if !self.teams.may(actor, team, TeamAction::CreateItems).await? {
            return Err(AppError::Forbidden);
        }
        let assigns_others = new
            .assignees
            .iter()
            .chain(&new.watchers)
            .any(|&person| person != actor);
        if assigns_others && !self.teams.may(actor, team, TeamAction::Assign).await? {
            return Err(AppError::Forbidden);
        }
        for (field, people) in [("assignees", &new.assignees), ("watchers", &new.watchers)] {
            for &person in people {
                if !self.teams.is_member(team, person).await? {
                    return Err(invalid(field, "not a member of this team"));
                }
            }
        }
        let now = self.clock.now();
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO items (id, kind, title, description, owner_team_id, team_visibility,
                                created_by, created_at, updated_at, last_touched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, $8)
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(&new.item.kind)
        .bind(&new.item.title)
        .bind(&new.item.description)
        .bind(team)
        .bind(new.visibility.as_str())
        .bind(actor)
        .bind(now)
        .fetch_one(&mut *conn)
        .await
        .map_err(internal)?;
        let tuples = std::iter::once(RelationTuple::new(id, Relation::OwnedBy, team))
            .chain(
                new.assignees
                    .iter()
                    .map(|&p| RelationTuple::new(id, Relation::Assignee, p)),
            )
            .chain(
                new.watchers
                    .iter()
                    .map(|&p| RelationTuple::new(id, Relation::Watcher, p)),
            );
        for tuple in tuples {
            self.access
                .grant(&mut *conn, tuple)
                .await
                .map_err(internal)?;
        }
        let event = NewActivity::new(Some(actor), id, ActivityKind::Created);
        activity::record(conn, now, event).await.map_err(internal)?;
        Ok(row.into())
    }

    /// Changes a team item's visibility mode (V12); access is recomputed in the same
    /// transaction, so people outside the new mode lose it at once (V8).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an item the actor cannot see, [`AppError::Forbidden`] without
    /// MANAGE, [`AppError::Validation`] for a personal item, [`AppError::Conflict`] for an item in
    /// the trash, or [`AppError::Internal`].
    pub async fn set_team_visibility(
        &self,
        actor: Uuid,
        id: Uuid,
        visibility: TeamVisibilityMode,
    ) -> Result<Item, AppError> {
        self.require(actor, id, Perms::MANAGE).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let row: Option<(Option<Uuid>, Option<String>, bool)> = sqlx::query_as(
            "SELECT owner_team_id, team_visibility, deleted_at IS NOT NULL
             FROM items WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let (team, current, deleted) = row.ok_or(AppError::NotFound)?;
        if team.is_none() {
            return Err(invalid(
                "visibility",
                "only team items have a visibility mode",
            ));
        }
        if deleted {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        if current.as_deref() != Some(visibility.as_str()) {
            sqlx::query("UPDATE items SET team_visibility = $2, updated_at = $3 WHERE id = $1")
                .bind(id)
                .bind(visibility.as_str())
                .bind(self.clock.now())
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            self.access
                .recompute(&mut tx, Change::Item(id))
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        self.load(id).await?.ok_or(AppError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: &str) -> CreateTeamItemRequest {
        CreateTeamItemRequest {
            kind: kind.into(),
            title: " Rapor ".into(),
            description: None,
            visibility: None,
            assignees: None,
            watchers: None,
        }
    }

    #[test]
    fn defaults_to_all_team() {
        let new = request("task").validate().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(new.visibility, TeamVisibilityMode::All);
        assert_eq!(new.item.title, "Rapor");
        assert!(new.assignees.is_empty() && new.watchers.is_empty());
    }

    #[test]
    fn people_are_deduplicated_and_assignees_win() {
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let new = CreateTeamItemRequest {
            assignees: Some(vec![a, a]),
            watchers: Some(vec![a, b, b]),
            visibility: Some(TeamVisibilityMode::Assignees),
            ..request("routine")
        }
        .validate()
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(new.assignees, [a]);
        assert_eq!(new.watchers, [b]);
        assert_eq!(new.visibility, TeamVisibilityMode::Assignees);
    }

    #[test]
    fn rejects_kinds_and_crowds() {
        assert!(request("notebook").validate().is_err());
        assert!(request("page").validate().is_err());
        assert!(request("nope").validate().is_err());
        let crowd = (0..=TEAM_ITEM_MAX_PEOPLE).map(|_| Uuid::now_v7()).collect();
        assert!(
            CreateTeamItemRequest {
                assignees: Some(crowd),
                ..request("task")
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn modes_round_trip() {
        for mode in [TeamVisibilityMode::All, TeamVisibilityMode::Assignees] {
            assert_eq!(TeamVisibilityMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(TeamVisibilityMode::parse("x"), None);
    }
}
