//! Shared fixtures for the visibility suite: relations go through the access engine (T1.1.3), checks
//! through `PermissionService` (T1.1.5). Grows with items (T1.2.1), friendships (T3.1.1), teams (T5.1.1).

use std::sync::Arc;

use access::{AccessConfig, AccessEngine, PermissionService, Relation, RelationTuple};
use db::testing::TestDb;
use items::{Item, ItemsAccessContext, ItemsService, NewItem};
use uuid::Uuid;

/// One isolated, fully migrated database per test.
pub(crate) struct World {
    db: TestDb,
    engine: AccessEngine<ItemsAccessContext>,
}

impl World {
    pub(crate) async fn new() -> Self {
        Self {
            db: TestDb::new().await,
            engine: AccessEngine::new(ItemsAccessContext, AccessConfig::default()),
        }
    }

    pub(crate) fn pool(&self) -> &db::PgPool {
        self.db.pool()
    }

    /// Database schema of this test; also its notification channel name.
    pub(crate) fn schema(&self) -> &str {
        self.db.schema()
    }

    pub(crate) async fn revoke(&self, tuple: RelationTuple) {
        let mut tx = self.pool().begin().await.unwrap();
        self.engine.revoke(&mut tx, tuple).await.unwrap();
        tx.commit().await.unwrap();
    }

    /// The items service on this world's database and access engine, with the system clock and
    /// the real team directory (T5.1.3).
    pub(crate) fn items(&self) -> ItemsService {
        ItemsService::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            self.engine.clone(),
        )
        .with_teams(Arc::new(self.teams()))
    }

    /// A real item of `kind` owned by `owner`.
    pub(crate) async fn item(&self, owner: Uuid, kind: &str, title: &str) -> Item {
        let new = NewItem {
            kind: kind.into(),
            title: title.into(),
            description: None,
        };
        self.items().create(owner, new).await.unwrap()
    }

    /// A team created by `admin`, with `members` joined in their roles (T5.1.1).
    pub(crate) async fn team(&self, admin: Uuid, members: &[(Uuid, teams::TeamRole)]) -> Uuid {
        let teams = self.teams();
        let team = teams
            .create(
                admin,
                teams::CreateTeamRequest {
                    name: "X".into(),
                    color: "#4f46e5".into(),
                    logo_url: None,
                },
            )
            .await
            .unwrap()
            .id;
        for (member, role) in members {
            teams
                .invite(
                    admin,
                    team,
                    teams::InviteToTeamRequest {
                        user_id: *member,
                        role: *role,
                    },
                )
                .await
                .unwrap();
            teams.accept(*member, team).await.unwrap();
        }
        team
    }

    /// A task owned by `team`, created by `creator` through the API with visibility `mode`
    /// (V12, T5.1.3).
    pub(crate) async fn team_task(
        &self,
        team: Uuid,
        creator: Uuid,
        mode: items::TeamVisibilityMode,
    ) -> Uuid {
        self.team_task_with(team, creator, mode, &[], &[]).await
    }

    /// [`Self::team_task`] with assignees and watchers chosen at creation (TK4).
    pub(crate) async fn team_task_with(
        &self,
        team: Uuid,
        creator: Uuid,
        mode: items::TeamVisibilityMode,
        assignees: &[Uuid],
        watchers: &[Uuid],
    ) -> Uuid {
        let new = items::CreateTeamItemRequest {
            kind: "task".into(),
            title: "Takım işi".into(),
            description: None,
            visibility: Some(mode),
            assignees: Some(assignees.to_vec()),
            watchers: Some(watchers.to_vec()),
        }
        .validate()
        .unwrap();
        self.items()
            .create_team_item(creator, team, new)
            .await
            .unwrap()
            .id
    }

    pub(crate) fn permissions(&self) -> PermissionService<ItemsAccessContext> {
        self.engine.permissions()
    }

    /// Writes relations through the access engine in one committed transaction.
    pub(crate) async fn grant(&self, tuples: &[RelationTuple]) {
        let mut tx = self.pool().begin().await.unwrap();
        for tuple in tuples {
            self.engine.grant(&mut tx, *tuple).await.unwrap();
        }
        tx.commit().await.unwrap();
    }
}

impl World {
    fn identity(&self) -> identity::IdentityService {
        identity::IdentityService::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            identity::EmailSettings {
                sender: Arc::new(kernel::email::RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        )
    }

    /// A registered person (friendships need real users); their id and e-mail address.
    pub(crate) async fn person(&self, name: &str) -> (Uuid, String) {
        let email = format!(
            "{}-{}@example.com",
            name.to_lowercase(),
            Uuid::now_v7().simple()
        );
        let request = identity::model::RegisterRequest {
            email: email.clone(),
            password: "correct horse".into(),
            display_name: name.into(),
            client: identity::model::ClientKind::Native,
            device: None,
            accepted_legal: identity::legal::current_acceptances(),
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity().register(request).await.unwrap();
        (user.id.as_uuid(), email)
    }

    pub(crate) fn friendships(&self) -> social::Friendships {
        social::Friendships::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            self.identity(),
        )
    }

    pub(crate) fn teams(&self) -> teams::TeamsService {
        teams::TeamsService::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            self.engine.clone(),
            self.identity(),
        )
    }

    /// The items service with the real V1 check: friends who did not block each other, or
    /// teammates, share (T5.1.1).
    pub(crate) fn sharing_items(&self) -> ItemsService {
        self.items().with_sharing(Arc::new(items::AnyOf(vec![
            Arc::new(self.friendships()),
            Arc::new(self.teams()),
        ])))
    }

    /// The notebooks service over [`Self::sharing_items`] (T3.4.1).
    pub(crate) fn notebooks(&self) -> notebooks::NotebooksService {
        notebooks::NotebooksService::new(
            self.pool().clone(),
            Arc::new(kernel::SystemClock),
            self.engine.clone(),
            self.sharing_items(),
        )
    }

    /// `owner`'s notebook with one page; their ids. `member` joins it with `role` through a real
    /// invitation, so `owner` and `member` must be friends (V1).
    pub(crate) async fn shared_notebook(
        &self,
        owner: Uuid,
        member: Uuid,
        role: items::InvitationRole,
    ) -> (Uuid, Uuid) {
        let notebooks = self.notebooks();
        let notebook = notebooks
            .create(
                owner,
                notebooks::CreateNotebookRequest {
                    title: "Defter".into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .item
            .id;
        let page = notebooks
            .create_page(
                owner,
                notebook,
                notebooks::CreatePageRequest { title: "P".into() },
            )
            .await
            .unwrap()
            .item
            .id;
        let items = self.sharing_items();
        let invitation = items
            .invite(
                owner,
                notebook,
                items::CreateInvitationRequest {
                    to_user: member,
                    role,
                },
            )
            .await
            .unwrap()
            .id;
        items.accept_invitation(member, invitation).await.unwrap();
        (notebook, page)
    }

    /// `a` asks `b` (by `b_email`) and `b` accepts.
    pub(crate) async fn befriend(&self, a: Uuid, b: Uuid, b_email: &str) {
        let social = self.friendships();
        social.send(a, b_email).await.unwrap();
        let request = social.incoming(b).await.unwrap()[0].id;
        social.accept(b, request).await.unwrap();
    }
}

pub(crate) fn id() -> Uuid {
    Uuid::now_v7()
}

pub(crate) fn t(object: Uuid, relation: Relation, subject: Uuid) -> RelationTuple {
    RelationTuple::new(object, relation, subject)
}

/// Body of a scenario whose behaviour is not implemented yet. The test is `#[ignore]`d with the
/// task that implements it; that task replaces this call with the real scenario.
pub(crate) fn pending() -> ! {
    panic!("scenario not implemented yet; see the #[ignore] reason for the task that enables it")
}
