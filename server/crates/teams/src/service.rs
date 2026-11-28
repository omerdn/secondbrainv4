//! Teams and membership (TK1, TK2, V1, T5.1.1).

use access::{AccessEngine, Relation, RelationTuple};
use chrono::{DateTime, Utc};
use identity::IdentityService;
use items::{
    ItemsAccessContext, SharingFuture, SharingPolicy, TeamAction, TeamDirectory, TeamFuture,
    TeamsFuture,
};
use kernel::{AppError, FieldError, FieldErrorCode, SharedClock, ValidationErrors};
use sqlx::PgPool;
use uuid::Uuid;

use crate::matrix::{
    RoleRights, TeamMatrix, TeamPermission, item_permissions, policy_of, team_rights,
};
use crate::model::{
    CreateTeamRequest, InviteToTeamByEmailRequest, InviteToTeamRequest, Team, TeamDetail,
    TeamInvitation, TeamInvitationCreated, TeamMember, TeamRole,
};

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn invalid(field: &str, message: &str) -> AppError {
    ValidationErrors::from(FieldError::new(field, FieldErrorCode::Invalid, message)).into()
}

#[derive(sqlx::FromRow)]
struct TeamRow {
    id: Uuid,
    name: String,
    color: String,
    logo_url: Option<String>,
    created_at: DateTime<Utc>,
    role: String,
}

fn to_team(row: TeamRow) -> Result<Team, AppError> {
    Ok(Team {
        role: TeamRole::parse(&row.role).ok_or_else(|| AppError::internal("unknown team role"))?,
        id: row.id,
        name: row.name,
        color: row.color,
        logo_url: row.logo_url,
        created_at: row.created_at,
    })
}

/// team id, team name, role, invited by, invited at.
type InvitationRow = (Uuid, String, String, Option<Uuid>, DateTime<Utc>);

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct TeamsService {
    pool: PgPool,
    clock: SharedClock,
    access: AccessEngine<ItemsAccessContext>,
    identity: IdentityService,
}

impl TeamsService {
    #[must_use]
    pub const fn new(
        pool: PgPool,
        clock: SharedClock,
        access: AccessEngine<ItemsAccessContext>,
        identity: IdentityService,
    ) -> Self {
        Self {
            pool,
            clock,
            access,
            identity,
        }
    }

    /// The caller's active role in `team`, if any.
    async fn role_of(&self, actor: Uuid, team: Uuid) -> Result<Option<TeamRole>, AppError> {
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(team)
        .bind(actor)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        Ok(role.as_deref().and_then(TeamRole::parse))
    }

    /// Creates a team; the creator is its admin (TK1).
    ///
    /// # Errors
    /// [`AppError::Validation`] or [`AppError::Internal`].
    pub async fn create(&self, actor: Uuid, request: CreateTeamRequest) -> Result<Team, AppError> {
        let request = request.validate()?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO teams (id, name, color, logo_url, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $6)",
        )
        .bind(id)
        .bind(&request.name)
        .bind(&request.color)
        .bind(&request.logo_url)
        .bind(actor)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id, role, status, invited_at, joined_at)
             VALUES ($1, $2, 'admin', 'active', $3, $3)",
        )
        .bind(id)
        .bind(actor)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        self.access
            .grant(
                &mut tx,
                RelationTuple::new(id, TeamRole::Admin.relation(), actor),
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(Team {
            id,
            name: request.name,
            color: request.color,
            logo_url: request.logo_url,
            created_at: now,
            role: TeamRole::Admin,
        })
    }

    /// The caller's teams (active memberships), by name.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn teams(&self, actor: Uuid) -> Result<Vec<Team>, AppError> {
        let rows: Vec<TeamRow> = sqlx::query_as(
            "SELECT t.id, t.name, t.color, t.logo_url, t.created_at, m.role
             FROM teams t JOIN team_members m ON m.team_id = t.id
             WHERE m.user_id = $1 AND m.status = 'active'
             ORDER BY lower(t.name), t.id",
        )
        .bind(actor)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        rows.into_iter().map(to_team).collect()
    }

    /// A team and its members, for its active members only; anyone else gets
    /// [`AppError::NotFound`] (no hint that it exists, V9).
    ///
    /// # Errors
    /// [`AppError::NotFound`] or [`AppError::Internal`].
    pub async fn team(&self, actor: Uuid, id: Uuid) -> Result<TeamDetail, AppError> {
        let row: Option<TeamRow> = sqlx::query_as(
            "SELECT t.id, t.name, t.color, t.logo_url, t.created_at, m.role
             FROM teams t JOIN team_members m ON m.team_id = t.id
             WHERE t.id = $1 AND m.user_id = $2 AND m.status = 'active'",
        )
        .bind(id)
        .bind(actor)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        let team = to_team(row.ok_or(AppError::NotFound)?)?;
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT user_id, role, status FROM team_members WHERE team_id = $1
             ORDER BY status, role, invited_at, user_id",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|(user, _, _)| *user).collect();
        let names = self.identity.display_names(&ids).await?;
        let mut members: Vec<TeamMember> = rows
            .into_iter()
            .filter_map(|(user_id, role, status)| {
                TeamRole::parse(&role).map(|role| TeamMember {
                    display_name: names.get(&user_id).cloned().unwrap_or_default(),
                    user_id,
                    role,
                    active: status == "active",
                })
            })
            .collect();
        members.sort_by(|a, b| {
            b.active
                .cmp(&a.active)
                .then(a.role.cmp(&b.role))
                .then(a.display_name.cmp(&b.display_name))
        });
        Ok(TeamDetail { team, members })
    }

    /// Invites someone with a role (TK2); admins and managers invite until the permission
    /// matrix exists (T5.1.2). No friendship is needed.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the caller is not in, [`AppError::Forbidden`] for a
    /// role that may not invite, [`AppError::Validation`] for oneself, an unknown person or an
    /// admin invitation by a manager, [`AppError::Conflict`] for a member or someone already
    /// invited, or [`AppError::Internal`].
    pub async fn invite(
        &self,
        actor: Uuid,
        team: Uuid,
        request: InviteToTeamRequest,
    ) -> Result<(), AppError> {
        let role = self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        if !self
            .allows(role, team, TeamPermission::InviteMembers)
            .await?
        {
            return Err(AppError::Forbidden);
        }
        if request.role == TeamRole::Admin && role != TeamRole::Admin {
            return Err(invalid("role", "only an admin invites admins"));
        }
        if request.user_id == actor {
            return Err(invalid("user_id", "you are already in the team"));
        }
        if self
            .identity
            .display_names(&[request.user_id])
            .await?
            .is_empty()
        {
            return Err(invalid("user_id", "no such person"));
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let inserted = sqlx::query(
            "INSERT INTO team_members (team_id, user_id, role, status, invited_by, invited_at)
             VALUES ($1, $2, $3, 'invited', $4, $5) ON CONFLICT DO NOTHING",
        )
        .bind(team)
        .bind(request.user_id)
        .bind(request.role.as_str())
        .bind(actor)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?
        .rows_affected();
        if inserted == 0 {
            return Err(AppError::Conflict(
                "already a member or invited to this team".into(),
            ));
        }
        db::outbox::publish(
            &mut tx,
            &TeamInvitationCreated {
                team_id: team,
                from_user: actor,
                to_user: request.user_id,
                role: request.role,
            },
        )
        .await
        .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Invites the owner of `request.email` (T5.2.6). The rights are checked first, and the
    /// answer is then the same whether the address has an account, is already in the team or
    /// was invited before: only a real new invitation reaches anyone (TK2, V9).
    ///
    /// # Errors
    /// As [`Self::invite`] before the address is looked up: [`AppError::NotFound`],
    /// [`AppError::Forbidden`], [`AppError::Validation`] for a bad address, one's own, or an admin
    /// invited by a non-admin; or [`AppError::Internal`].
    pub async fn invite_by_email(
        &self,
        actor: Uuid,
        team: Uuid,
        request: InviteToTeamByEmailRequest,
    ) -> Result<(), AppError> {
        let role = self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        if !self
            .allows(role, team, TeamPermission::InviteMembers)
            .await?
        {
            return Err(AppError::Forbidden);
        }
        if request.role == TeamRole::Admin && role != TeamRole::Admin {
            return Err(invalid("role", "only an admin invites admins"));
        }
        let email = request.email.trim().to_lowercase();
        identity::model::check_email("email", &email).map_err(ValidationErrors::from)?;
        let Some(user_id) = self.identity.user_id_by_email(&email).await? else {
            return Ok(());
        };
        if user_id == actor {
            return Err(invalid("email", "you are already in the team"));
        }
        let invited = self
            .invite(
                actor,
                team,
                InviteToTeamRequest {
                    user_id,
                    role: request.role,
                },
            )
            .await;
        match invited {
            Ok(()) | Err(AppError::Conflict(_)) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// The caller's team invitations, newest first.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn invitations(&self, actor: Uuid) -> Result<Vec<TeamInvitation>, AppError> {
        let rows: Vec<InvitationRow> = sqlx::query_as(
            "SELECT t.id, t.name, m.role, m.invited_by, m.invited_at
             FROM team_members m JOIN teams t ON t.id = m.team_id
             WHERE m.user_id = $1 AND m.status = 'invited'
             ORDER BY m.invited_at DESC, t.id",
        )
        .bind(actor)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .filter_map(|(team_id, team_name, role, invited_by, invited_at)| {
                TeamRole::parse(&role).map(|role| TeamInvitation {
                    team_id,
                    team_name,
                    role,
                    invited_by,
                    invited_at,
                })
            })
            .collect())
    }

    /// Accepts the caller's invitation: they join with its role (TK2).
    ///
    /// # Errors
    /// [`AppError::NotFound`] without a pending invitation, or [`AppError::Internal`].
    pub async fn accept(&self, actor: Uuid, team: Uuid) -> Result<Team, AppError> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let role: Option<String> = sqlx::query_scalar(
            "UPDATE team_members SET status = 'active', joined_at = $3
             WHERE team_id = $1 AND user_id = $2 AND status = 'invited' RETURNING role",
        )
        .bind(team)
        .bind(actor)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let role = role
            .as_deref()
            .and_then(TeamRole::parse)
            .ok_or(AppError::NotFound)?;
        self.access
            .grant(&mut tx, RelationTuple::new(team, role.relation(), actor))
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(self.team(actor, team).await?.team)
    }

    /// Declines the caller's invitation; nothing remains of it.
    ///
    /// # Errors
    /// [`AppError::NotFound`] without a pending invitation, or [`AppError::Internal`].
    pub async fn decline(&self, actor: Uuid, team: Uuid) -> Result<(), AppError> {
        let gone = sqlx::query(
            "DELETE FROM team_members WHERE team_id = $1 AND user_id = $2 AND status = 'invited'",
        )
        .bind(team)
        .bind(actor)
        .execute(&self.pool)
        .await
        .map_err(internal)?
        .rows_affected();
        if gone == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// A team's name for whoever is in it or invited to it (the notification center).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anyone else, or [`AppError::Internal`].
    pub async fn name_for(&self, user: Uuid, team: Uuid) -> Result<String, AppError> {
        sqlx::query_scalar(
            "SELECT t.name FROM teams t JOIN team_members m ON m.team_id = t.id
             WHERE t.id = $1 AND m.user_id = $2",
        )
        .bind(team)
        .bind(user)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or(AppError::NotFound)
    }

    /// The team permissions of `role` in `team` (TK3): admins have all, others the stored row
    /// or the defaults.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn rights(
        &self,
        role: TeamRole,
        team: Uuid,
    ) -> Result<Vec<TeamPermission>, AppError> {
        if role == TeamRole::Admin {
            return Ok(TeamPermission::ALL.to_vec());
        }
        let stored: Option<Vec<String>> = sqlx::query_scalar(
            "SELECT permissions FROM team_permissions WHERE team_id = $1 AND role = $2",
        )
        .bind(team)
        .bind(role.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        Ok(match stored {
            Some(names) => names
                .iter()
                .filter_map(|n| TeamPermission::parse(n))
                .collect(),
            None => TeamPermission::defaults(role),
        })
    }

    async fn allows(
        &self,
        role: TeamRole,
        team: Uuid,
        permission: TeamPermission,
    ) -> Result<bool, AppError> {
        Ok(self.rights(role, team).await?.contains(&permission))
    }

    /// Whether `actor` is an active member whose role has `permission` in `team` (TK3), for the
    /// modules that act in teams (team items, tags, notebooks).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn may(
        &self,
        actor: Uuid,
        team: Uuid,
        permission: TeamPermission,
    ) -> Result<bool, AppError> {
        match self.role_of(actor, team).await? {
            Some(role) => self.allows(role, team, permission).await,
            None => Ok(false),
        }
    }

    /// The team's matrix, for its active members (TK3).
    ///
    /// # Errors
    /// [`AppError::NotFound`] for anyone else, or [`AppError::Internal`].
    pub async fn matrix(&self, actor: Uuid, team: Uuid) -> Result<TeamMatrix, AppError> {
        self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let policy = self
            .access
            .team_policy(&mut conn, team)
            .await
            .map_err(internal)?;
        drop(conn);
        let mut roles = Vec::new();
        for role in [
            TeamRole::Admin,
            TeamRole::Manager,
            TeamRole::Member,
            TeamRole::Guest,
        ] {
            roles.push(RoleRights {
                role,
                item: item_permissions(policy.perms(role.access_role())),
                team: self.rights(role, team).await?,
            });
        }
        Ok(TeamMatrix { roles })
    }

    /// Replaces the matrix (admins only, TK3). Item rights recompute the team's items at once
    /// (V8); the admin row is ignored.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the caller is not in, [`AppError::Forbidden`] for a
    /// non-admin, or [`AppError::Internal`].
    pub async fn set_matrix(
        &self,
        actor: Uuid,
        team: Uuid,
        matrix: TeamMatrix,
    ) -> Result<TeamMatrix, AppError> {
        let role = self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        if role != TeamRole::Admin {
            return Err(AppError::Forbidden);
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let current = self
            .access
            .team_policy(&mut tx, team)
            .await
            .map_err(internal)?;
        self.access
            .set_team_policy(&mut tx, team, policy_of(&matrix, current))
            .await
            .map_err(internal)?;
        for role in [TeamRole::Manager, TeamRole::Member, TeamRole::Guest] {
            if let Some(rights) = team_rights(&matrix, role) {
                let names: Vec<&str> = rights.iter().map(|p| p.as_str()).collect();
                sqlx::query(
                    "INSERT INTO team_permissions (team_id, role, permissions) VALUES ($1, $2, $3)
                     ON CONFLICT (team_id, role) DO UPDATE SET permissions = $3",
                )
                .bind(team)
                .bind(role.as_str())
                .bind(&names)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            }
        }
        tx.commit().await.map_err(internal)?;
        self.matrix(actor, team).await
    }

    /// Gives an active member another role (admins only, TK3); access follows at once (V8). The
    /// last admin cannot step down.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the caller is not in or someone not active in it,
    /// [`AppError::Forbidden`] for a non-admin, [`AppError::Validation`] for the last admin, or
    /// [`AppError::Internal`].
    pub async fn set_role(
        &self,
        actor: Uuid,
        team: Uuid,
        member: Uuid,
        role: TeamRole,
    ) -> Result<(), AppError> {
        let mine = self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        if mine != TeamRole::Admin {
            return Err(AppError::Forbidden);
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Serialises role changes of one team (the last-admin check).
        sqlx::query("SELECT id FROM teams WHERE id = $1 FOR UPDATE")
            .bind(team)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let old: Option<String> = sqlx::query_scalar(
            "SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2 AND status = 'active'",
        )
        .bind(team)
        .bind(member)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let old = old
            .as_deref()
            .and_then(TeamRole::parse)
            .ok_or(AppError::NotFound)?;
        if old == role {
            return Ok(());
        }
        if old == TeamRole::Admin {
            let admins: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM team_members
                 WHERE team_id = $1 AND role = 'admin' AND status = 'active'",
            )
            .bind(team)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if admins <= 1 {
                return Err(invalid("role", "a team keeps at least one admin"));
            }
        }
        sqlx::query("UPDATE team_members SET role = $3 WHERE team_id = $1 AND user_id = $2")
            .bind(team)
            .bind(member)
            .bind(role.as_str())
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        self.access
            .grant(&mut tx, RelationTuple::new(team, role.relation(), member))
            .await
            .map_err(internal)?;
        self.access
            .revoke(&mut tx, RelationTuple::new(team, old.relation(), member))
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// Leaves a team (TK7): the team's items stay in the team, and every way the person reached
    /// them through it drops at once, open sessions included (V8, V10). The last admin cannot
    /// leave.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the caller is not an active member of,
    /// [`AppError::Validation`] for the last admin, or [`AppError::Internal`].
    pub async fn leave(&self, actor: Uuid, team: Uuid) -> Result<(), AppError> {
        self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        self.drop_member(team, actor).await
    }

    /// Removes someone from a team (TK5, TK7), with the same effect as leaving. Needs "invite
    /// members" (TK3); only admins remove admins and managers. Oneself leaves with
    /// [`Self::leave`].
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a team the caller is not in or someone not active in it,
    /// [`AppError::Forbidden`] without the right, [`AppError::Validation`] for oneself or the
    /// last admin, or [`AppError::Internal`].
    pub async fn remove_member(
        &self,
        actor: Uuid,
        team: Uuid,
        member: Uuid,
    ) -> Result<(), AppError> {
        let mine = self.role_of(actor, team).await?.ok_or(AppError::NotFound)?;
        if !self
            .allows(mine, team, TeamPermission::InviteMembers)
            .await?
        {
            return Err(AppError::Forbidden);
        }
        if member == actor {
            return Err(invalid("user_id", "leave the team instead"));
        }
        let theirs = self
            .role_of(member, team)
            .await?
            .ok_or(AppError::NotFound)?;
        if matches!(theirs, TeamRole::Admin | TeamRole::Manager) && mine != TeamRole::Admin {
            return Err(AppError::Forbidden);
        }
        self.drop_member(team, member).await
    }

    /// Ends `member`'s active membership: their role relation and their direct relations to the
    /// team's items (assignee, watcher, notebook editor or viewer) go in one transaction, so
    /// access is recomputed and revoked at once.
    async fn drop_member(&self, team: Uuid, member: Uuid) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // Serialises membership changes of one team (the last-admin check).
        sqlx::query("SELECT id FROM teams WHERE id = $1 FOR UPDATE")
            .bind(team)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        let role: Option<String> = sqlx::query_scalar(
            "DELETE FROM team_members WHERE team_id = $1 AND user_id = $2 AND status = 'active'
             RETURNING role",
        )
        .bind(team)
        .bind(member)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        let role = role
            .as_deref()
            .and_then(TeamRole::parse)
            .ok_or(AppError::NotFound)?;
        if role == TeamRole::Admin {
            let admins: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM team_members
                 WHERE team_id = $1 AND role = 'admin' AND status = 'active'",
            )
            .bind(team)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if admins == 0 {
                return Err(invalid("user_id", "a team keeps at least one admin"));
            }
        }
        self.access
            .revoke(&mut tx, RelationTuple::new(team, role.relation(), member))
            .await
            .map_err(internal)?;
        let owned: std::collections::HashSet<Uuid> = self
            .access
            .items_of_team(&mut *tx, team)
            .await
            .map_err(internal)?
            .into_iter()
            .collect();
        let direct = self
            .access
            .relations_of_user(&mut *tx, member)
            .await
            .map_err(internal)?;
        for tuple in direct {
            let personal = matches!(
                tuple.relation(),
                Relation::Assignee | Relation::Watcher | Relation::Editor | Relation::Viewer
            );
            if personal && owned.contains(&tuple.object_id()) {
                self.access.revoke(&mut tx, tuple).await.map_err(internal)?;
            }
        }
        tx.commit().await.map_err(internal)
    }

    /// The teams `user` is an active member of.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn teams_of(&self, user: Uuid) -> Result<Vec<Uuid>, AppError> {
        sqlx::query_scalar(
            "SELECT team_id FROM team_members WHERE user_id = $1 AND status = 'active'",
        )
        .bind(user)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)
    }

    /// Whether `user` is an active member of `team`.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn is_member(&self, team: Uuid, user: Uuid) -> Result<bool, AppError> {
        Ok(self.role_of(user, team).await?.is_some())
    }

    /// Whether two people are active members of a common team (V1).
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn teammates(&self, a: Uuid, b: Uuid) -> Result<bool, AppError> {
        sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM team_members x JOIN team_members y ON y.team_id = x.team_id
                 WHERE x.user_id = $1 AND y.user_id = $2
                   AND x.status = 'active' AND y.status = 'active')",
        )
        .bind(a)
        .bind(b)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)
    }
}

/// V1: teammates share without being friends.
impl SharingPolicy for TeamsService {
    fn can_share(&self, from: Uuid, to: Uuid) -> SharingFuture<'_> {
        Box::pin(self.teammates(from, to))
    }
}

/// Team items and tags (T5.1.3, T5.1.4): who belongs to a team and what their role may do.
impl TeamDirectory for TeamsService {
    fn teams_of(&self, user: Uuid) -> TeamsFuture<'_> {
        Box::pin(Self::teams_of(self, user))
    }

    fn is_member(&self, team: Uuid, user: Uuid) -> TeamFuture<'_> {
        Box::pin(Self::is_member(self, team, user))
    }

    fn may(&self, actor: Uuid, team: Uuid, action: TeamAction) -> TeamFuture<'_> {
        let permission = match action {
            TeamAction::CreateItems => TeamPermission::CreateItems,
            TeamAction::Assign => TeamPermission::Assign,
            TeamAction::ManageTags => TeamPermission::ManageTags,
            TeamAction::CreateNotebooks => TeamPermission::CreateNotebooks,
        };
        Box::pin(Self::may(self, actor, team, permission))
    }
}
