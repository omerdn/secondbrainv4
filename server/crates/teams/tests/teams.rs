//! Teams and membership (T5.1.1, PRD TK1, TK2, V9) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, Relation};
use chrono::{TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::ItemsAccessContext;
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use teams::{CreateTeamRequest, InviteToTeamRequest, TeamRole, TeamsService};
use uuid::Uuid;

struct World {
    db: TestDb,
    identity: IdentityService,
    teams: TeamsService,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let clock: kernel::SharedClock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 28, 9, 0, 0).unwrap(),
        ));
        let identity = IdentityService::new(
            db.pool().clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "https://app.test".into(),
            },
        );
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let teams = TeamsService::new(db.pool().clone(), clock, access, identity.clone());
        Self {
            db,
            identity,
            teams,
        }
    }

    async fn user(&self, email: &str, name: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: name.into(),
            client: ClientKind::Native,
            device: None,
            accepted_legal: identity::legal::current_acceptances(),
        }
        .validate()
        .unwrap();
        self.identity
            .register(request)
            .await
            .unwrap()
            .0
            .id
            .as_uuid()
    }

    async fn team(&self, admin: Uuid) -> Uuid {
        self.teams
            .create(
                admin,
                CreateTeamRequest {
                    name: "Ev".into(),
                    color: "#4f46e5".into(),
                    logo_url: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    async fn invite(
        &self,
        actor: Uuid,
        team: Uuid,
        to: Uuid,
        role: TeamRole,
    ) -> Result<(), AppError> {
        self.teams
            .invite(actor, team, InviteToTeamRequest { user_id: to, role })
            .await
    }

    async fn relation(&self, team: Uuid, user: Uuid) -> Option<String> {
        sqlx::query_scalar(
            "SELECT relation FROM access_relations
             WHERE object_type = 'team' AND object_id = $1 AND subject_id = $2",
        )
        .bind(team)
        .bind(user)
        .fetch_optional(self.db.pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn tk1_tk2_create_invite_and_accept() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let team = w.team(ada).await;
    assert_eq!(w.teams.teams(ada).await.unwrap()[0].role, TeamRole::Admin);
    assert_eq!(
        w.relation(team, ada).await.as_deref(),
        Some(Relation::TeamAdmin.as_str())
    );

    w.invite(ada, team, bob, TeamRole::Member).await.unwrap();
    // Invited is not joined: no team, no role, no detail (V9).
    assert!(w.teams.teams(bob).await.unwrap().is_empty());
    assert_eq!(w.relation(team, bob).await, None);
    assert!(matches!(
        w.teams.team(bob, team).await,
        Err(AppError::NotFound)
    ));
    let invitations = w.teams.invitations(bob).await.unwrap();
    assert_eq!(invitations.len(), 1);
    assert_eq!(invitations[0].team_name, "Ev");
    assert_eq!(invitations[0].invited_by, Some(ada));
    // The admin sees the invitation among the members, not active.
    let detail = w.teams.team(ada, team).await.unwrap();
    assert_eq!(detail.members.len(), 2);
    assert!(detail.members.iter().any(|m| m.user_id == bob && !m.active));
    // A team invitation needs no friendship, and an event is published for the notification.
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox WHERE event_type = 'team.invitation_created'",
    )
    .fetch_one(w.db.pool())
    .await
    .unwrap();
    assert_eq!(events, 1);

    let joined = w.teams.accept(bob, team).await.unwrap();
    assert_eq!(joined.role, TeamRole::Member);
    assert_eq!(w.relation(team, bob).await.as_deref(), Some("member"));
    assert!(w.teams.invitations(bob).await.unwrap().is_empty());
    assert!(w.teams.teammates(ada, bob).await.unwrap());
    // Accepting twice finds nothing.
    assert!(matches!(
        w.teams.accept(bob, team).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn tk2_declining_leaves_nothing() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let team = w.team(ada).await;
    w.invite(ada, team, bob, TeamRole::Guest).await.unwrap();
    w.teams.decline(bob, team).await.unwrap();
    assert!(w.teams.invitations(bob).await.unwrap().is_empty());
    assert!(!w.teams.teammates(ada, bob).await.unwrap());
    assert!(matches!(
        w.teams.accept(bob, team).await,
        Err(AppError::NotFound)
    ));
    // Ada may invite him again.
    w.invite(ada, team, bob, TeamRole::Member).await.unwrap();
}

#[tokio::test]
async fn only_admins_and_managers_invite_and_only_admins_invite_admins() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let eve = w.user("eve@example.com", "Eve").await;
    let team = w.team(ada).await;
    // Outsiders do not learn the team exists.
    assert!(matches!(
        w.invite(eve, team, can, TeamRole::Member).await,
        Err(AppError::NotFound)
    ));
    w.invite(ada, team, bob, TeamRole::Member).await.unwrap();
    w.teams.accept(bob, team).await.unwrap();
    assert!(matches!(
        w.invite(bob, team, can, TeamRole::Member).await,
        Err(AppError::Forbidden)
    ));
    w.invite(ada, team, can, TeamRole::Manager).await.unwrap();
    w.teams.accept(can, team).await.unwrap();
    assert!(matches!(
        w.invite(can, team, eve, TeamRole::Admin).await,
        Err(AppError::Validation(_))
    ));
    w.invite(can, team, eve, TeamRole::Member).await.unwrap();
    // Twice, a member, or oneself.
    assert!(matches!(
        w.invite(can, team, eve, TeamRole::Member).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.invite(ada, team, bob, TeamRole::Member).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        w.invite(ada, team, ada, TeamRole::Member).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.invite(ada, team, Uuid::now_v7(), TeamRole::Member).await,
        Err(AppError::Validation(_))
    ));
}

// --- T5.1.2: roles and the permission matrix --------------------------------------------------

#[tokio::test]
async fn tk3_the_matrix_decides_who_invites_and_only_admins_edit_it() {
    use teams::{TeamPermission, TeamRole};
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let can = w.user("can@example.com", "Can").await;
    let eve = w.user("eve@example.com", "Eve").await;
    let team = w.team(ada).await;
    w.invite(ada, team, bob, TeamRole::Member).await.unwrap();
    w.teams.accept(bob, team).await.unwrap();
    w.invite(ada, team, can, TeamRole::Manager).await.unwrap();
    w.teams.accept(can, team).await.unwrap();

    // The defaults: every member sees the matrix; admins have everything.
    let mut matrix = w.teams.matrix(bob, team).await.unwrap();
    assert_eq!(matrix.roles[0].role, TeamRole::Admin);
    assert_eq!(matrix.roles[0].team.len(), TeamPermission::ALL.len());
    assert!(
        !w.teams
            .may(bob, team, TeamPermission::InviteMembers)
            .await
            .unwrap()
    );
    assert!(matches!(
        w.teams.set_matrix(can, team, matrix.clone()).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.teams.matrix(eve, team).await,
        Err(AppError::NotFound)
    ));

    // Members may invite; managers may not.
    for row in &mut matrix.roles {
        match row.role {
            TeamRole::Member => row.team.push(TeamPermission::InviteMembers),
            TeamRole::Manager => row.team.retain(|p| *p != TeamPermission::InviteMembers),
            // Ignored: admins keep everything.
            TeamRole::Admin => row.team.clear(),
            TeamRole::Guest => {}
        }
    }
    let saved = w.teams.set_matrix(ada, team, matrix).await.unwrap();
    assert_eq!(saved.roles[0].team.len(), TeamPermission::ALL.len());
    assert!(matches!(
        w.invite(can, team, eve, TeamRole::Member).await,
        Err(AppError::Forbidden)
    ));
    w.invite(bob, team, eve, TeamRole::Member).await.unwrap();
    assert!(
        w.teams
            .may(ada, team, TeamPermission::InviteMembers)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn tk3_v8_admins_change_roles_but_a_team_keeps_an_admin() {
    use teams::TeamRole;
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let team = w.team(ada).await;
    w.invite(ada, team, bob, TeamRole::Member).await.unwrap();
    w.teams.accept(bob, team).await.unwrap();
    assert!(matches!(
        w.teams.set_role(bob, team, ada, TeamRole::Guest).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        w.teams.set_role(ada, team, ada, TeamRole::Member).await,
        Err(AppError::Validation(_))
    ));
    w.teams
        .set_role(ada, team, bob, TeamRole::Admin)
        .await
        .unwrap();
    assert_eq!(w.relation(team, bob).await.as_deref(), Some("admin"));
    // Now Ada may step down.
    w.teams
        .set_role(ada, team, ada, TeamRole::Member)
        .await
        .unwrap();
    assert_eq!(w.relation(team, ada).await.as_deref(), Some("member"));
    assert!(matches!(
        w.teams
            .set_role(bob, team, Uuid::now_v7(), TeamRole::Member)
            .await,
        Err(AppError::NotFound)
    ));
}
