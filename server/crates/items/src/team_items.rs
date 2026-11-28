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

/// Someone chose a teammate for a team item, at creation or later (TK2, TK4, T5.1.8); published
/// in that transaction. Choosing oneself publishes nothing. The notification center tells the
/// person (BL2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberAdded {
    pub item_id: Uuid,
    pub assigned_by: Uuid,
    pub user_id: Uuid,
    pub role: InvitationRole,
}

impl kernel::DomainEvent for TeamMemberAdded {
    const EVENT_TYPE: &'static str = "item.team_member_added";
}

/// `PUT /items/{id}/members/{user_id}` body: a team item's member, added directly (TK2).
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct AddTeamMemberRequest {
    pub role: InvitationRole,
}

const fn role_of(relation: Relation) -> InvitationRole {
    match relation {
        Relation::Watcher => InvitationRole::Watcher,
        Relation::Editor => InvitationRole::Editor,
        Relation::Viewer => InvitationRole::Viewer,
        _ => InvitationRole::Assignee,
    }
}

const fn relation_of(role: InvitationRole) -> Relation {
    match role {
        InvitationRole::Assignee => Relation::Assignee,
        InvitationRole::Watcher => Relation::Watcher,
        InvitationRole::Editor => Relation::Editor,
        InvitationRole::Viewer => Relation::Viewer,
    }
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
    /// Create a team notebook (Y2, T5.1.5).
    CreateNotebooks,
    /// Create, rename, recolor and delete the team's tags (E3, T5.1.4).
    ManageTags,
}

/// Result of a [`TeamDirectory`] check.
pub type TeamFuture<'a> = Pin<Box<dyn Future<Output = Result<bool, AppError>> + Send + 'a>>;

/// Result of [`TeamDirectory::teams_of`].
pub type TeamsFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<Uuid>, AppError>> + Send + 'a>>;

/// What `items` needs to know about teams; implemented by the `teams` module.
pub trait TeamDirectory: Send + Sync {
    /// The teams `user` is an active member of.
    fn teams_of(&self, user: Uuid) -> TeamsFuture<'_>;
    /// Whether `user` is an active member of `team`.
    fn is_member(&self, team: Uuid, user: Uuid) -> TeamFuture<'_>;
    /// Whether `actor` is an active member whose role allows `action` in `team`.
    fn may(&self, actor: Uuid, team: Uuid, action: TeamAction) -> TeamFuture<'_>;
}

/// Knows no team. The default until the `teams` module is wired in.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTeams;

impl TeamDirectory for NoTeams {
    fn teams_of(&self, _user: Uuid) -> TeamsFuture<'_> {
        Box::pin(async { Ok(Vec::new()) })
    }
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

/// Where a new task, routine, goal or tracker goes when its team is chosen (T5.1.8): the
/// `team` field of their create requests. Absent, the item is personal.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct TeamPlacementRequest {
    pub team_id: Uuid,
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

/// A validated new team item: no person twice (an assignee is not also a watcher). Notebooks
/// have editors and viewers instead (Y2, T5.1.5); other kinds leave those empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTeamItem {
    pub item: NewItem,
    pub visibility: TeamVisibilityMode,
    pub assignees: Vec<Uuid>,
    pub watchers: Vec<Uuid>,
    pub editors: Vec<Uuid>,
    pub viewers: Vec<Uuid>,
}

impl NewTeamItem {
    /// The people chosen for the item, each with their relation.
    fn people(&self) -> impl Iterator<Item = (Uuid, Relation)> + '_ {
        [
            (&self.assignees, Relation::Assignee),
            (&self.watchers, Relation::Watcher),
            (&self.editors, Relation::Editor),
            (&self.viewers, Relation::Viewer),
        ]
        .into_iter()
        .flat_map(|(people, relation)| people.iter().map(move |&p| (p, relation)))
    }
}

/// `people` without duplicates and without anyone in `skip`, in order: an earlier list wins
/// (assignees over watchers, editors over viewers).
#[must_use]
pub fn unique_people(people: Option<Vec<Uuid>>, skip: &[Uuid]) -> Vec<Uuid> {
    unique(people, skip)
}

/// `PUT /items/{id}/visibility` body.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export)]
pub struct SetTeamVisibilityRequest {
    pub visibility: TeamVisibilityMode,
}

/// The request field that names people of `relation`.
const fn field_of(relation: Relation) -> &'static str {
    match relation {
        Relation::Watcher => "watchers",
        Relation::Editor => "editors",
        Relation::Viewer => "viewers",
        _ => "assignees",
    }
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
            editors: Vec::new(),
            viewers: Vec::new(),
        })
    }
}

impl ItemsService {
    /// Creates `new` inside the caller's transaction: personal without `team`, otherwise owned
    /// by the chosen team with its visibility and people (V12, TK4). For the modules whose
    /// detail row must exist together with the item (T5.1.8).
    ///
    /// # Errors
    /// As [`Self::create_in`] or [`Self::create_team_item_in`], and [`AppError::Validation`]
    /// for a placement the kind does not allow.
    pub async fn create_placed_in(
        &self,
        conn: &mut sqlx::PgConnection,
        actor: Uuid,
        new: NewItem,
        team: Option<TeamPlacementRequest>,
    ) -> Result<Item, AppError> {
        let Some(team) = team else {
            return self.create_in(conn, actor, new).await;
        };
        let placed = CreateTeamItemRequest {
            kind: new.kind,
            title: new.title,
            description: new.description,
            visibility: team.visibility,
            assignees: team.assignees,
            watchers: team.watchers,
        }
        .validate()?;
        self.create_team_item_in(conn, actor, team.team_id, placed)
            .await
    }

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
        if new.item.kind == "page" {
            return Err(invalid("kind", "team pages are created in a team notebook"));
        }
        let action = if new.item.kind == "notebook" {
            TeamAction::CreateNotebooks
        } else {
            TeamAction::CreateItems
        };
        if !self.teams.may(actor, team, action).await? {
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
        for (person, relation) in new.people() {
            if !self.teams.is_member(team, person).await? {
                return Err(invalid(field_of(relation), "not a member of this team"));
            }
        }
        let item = self
            .insert_team_item(&mut *conn, actor, team, &new.item, new.visibility)
            .await?;
        for (person, relation) in new.people() {
            self.add_member_in(&mut *conn, actor, item.id, person, relation)
                .await?;
        }
        Ok(item)
    }

    /// Adds a teammate to a team item directly, without an invitation (TK2, T5.1.8): an
    /// assignee or watcher, or a notebook's editor or viewer. Choosing someone else needs SHARE
    /// on the item (V5: the role matrix); choosing oneself needs EDIT. Adding an existing member
    /// again is a no-op.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an item the actor cannot see, [`AppError::Forbidden`] without
    /// the right, [`AppError::Validation`] for a personal item (it invites), a role the kind
    /// does not have or someone outside the team, [`AppError::Conflict`] for an item in the
    /// trash, or [`AppError::Internal`].
    pub async fn add_team_member(
        &self,
        actor: Uuid,
        item_id: Uuid,
        user: Uuid,
        role: InvitationRole,
    ) -> Result<(), AppError> {
        self.require(actor, item_id, Perms::VIEW).await?;
        let needed = if user == actor {
            Perms::EDIT
        } else {
            Perms::SHARE
        };
        self.require(actor, item_id, needed).await?;
        let item = self.load(item_id).await?.ok_or(AppError::NotFound)?;
        let crate::model::Owner::Team { team_id } = item.owner else {
            return Err(invalid("user_id", "personal items invite people instead"));
        };
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        if !role_allowed(&item.kind, role) {
            return Err(invalid("role", "this item kind has no such role"));
        }
        if !self.teams.is_member(team_id, user).await? {
            return Err(invalid("user_id", "not a member of this team"));
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        self.add_member_in(&mut tx, actor, item_id, user, relation_of(role))
            .await?;
        tx.commit().await.map_err(internal)
    }

    async fn add_member_in(
        &self,
        conn: &mut sqlx::PgConnection,
        actor: Uuid,
        item: Uuid,
        user: Uuid,
        relation: Relation,
    ) -> Result<(), AppError> {
        let added = self
            .access
            .grant(&mut *conn, RelationTuple::new(item, relation, user))
            .await
            .map_err(internal)?;
        if added && user != actor {
            let event = TeamMemberAdded {
                item_id: item,
                assigned_by: actor,
                user_id: user,
                role: role_of(relation),
            };
            db::outbox::publish(&mut *conn, &event)
                .await
                .map_err(internal)?;
        }
        Ok(())
    }

    /// A page in a team notebook, owned by the team (Y5). The caller has checked EDIT on the
    /// notebook and links the page to it; its readers come from the notebook. On the page itself
    /// only managers and above hold their role (see `ItemsAccessContext`), so a role allowed to
    /// link or delete team pages does so here (V4, V6, T5.1.5).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn create_team_page_in(
        &self,
        conn: &mut sqlx::PgConnection,
        actor: Uuid,
        team: Uuid,
        new: NewItem,
    ) -> Result<Item, AppError> {
        self.insert_team_item(conn, actor, team, &new, TeamVisibilityMode::Assignees)
            .await
    }

    async fn insert_team_item(
        &self,
        conn: &mut sqlx::PgConnection,
        actor: Uuid,
        team: Uuid,
        new: &NewItem,
        visibility: TeamVisibilityMode,
    ) -> Result<Item, AppError> {
        let now = self.clock.now();
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        let row = sqlx::query_as::<_, ItemRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO items (id, kind, title, description, owner_team_id, team_visibility,
                                created_by, created_at, updated_at, last_touched_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, $8)
             RETURNING {COLUMNS}"
        )))
        .bind(id)
        .bind(&new.kind)
        .bind(&new.title)
        .bind(&new.description)
        .bind(team)
        .bind(visibility.as_str())
        .bind(actor)
        .bind(now)
        .fetch_one(&mut *conn)
        .await
        .map_err(internal)?;
        self.access
            .grant(&mut *conn, RelationTuple::new(id, Relation::OwnedBy, team))
            .await
            .map_err(internal)?;
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
