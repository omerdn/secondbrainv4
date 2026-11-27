//! Friendships and blocks (T3.1.1, PRD AK1, AK2, V1) against a real Postgres 17. Requires Docker
//! (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use access::{AccessConfig, AccessEngine};
use chrono::{Duration, TimeZone, Utc};
use db::testing::TestDb;
use identity::model::{ClientKind, RegisterRequest};
use identity::{EmailSettings, IdentityService};
use items::{CreateInvitationRequest, InvitationRole, ItemsAccessContext, ItemsService, NewItem};
use kernel::email::RecordingEmailSender;
use kernel::{AppError, FixedClock};
use social::{DECLINE_COOLDOWN, Friendships, REQUESTS_PER_DAY};
use uuid::Uuid;

struct World {
    _db: TestDb,
    clock: Arc<FixedClock>,
    identity: IdentityService,
    items: ItemsService,
    social: Friendships,
}

impl World {
    async fn new() -> Self {
        let db = TestDb::new().await;
        let pool = db.pool().clone();
        let clock = Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap(),
        ));
        let identity = IdentityService::new(
            pool.clone(),
            clock.clone(),
            EmailSettings {
                sender: Arc::new(RecordingEmailSender::default()),
                web_url: "http://localhost:8081".into(),
            },
        );
        let social = Friendships::new(pool.clone(), clock.clone(), identity.clone());
        let access = AccessEngine::new(ItemsAccessContext, AccessConfig::default());
        let items =
            ItemsService::new(pool, clock.clone(), access).with_sharing(Arc::new(social.clone()));
        Self {
            _db: db,
            clock,
            identity,
            items,
            social,
        }
    }

    async fn user(&self, email: &str, name: &str) -> Uuid {
        let request = RegisterRequest {
            email: email.into(),
            password: "correct horse".into(),
            display_name: name.into(),
            client: ClientKind::Native,
            device: None,
        }
        .validate()
        .unwrap();
        let (user, _) = self.identity.register(request).await.unwrap();
        user.id.as_uuid()
    }

    /// Sends a request and has it accepted.
    async fn befriend(&self, a: Uuid, b: Uuid, b_email: &str) {
        self.social.send(a, b_email).await.unwrap();
        let id = self.social.incoming(b).await.unwrap()[0].id;
        self.social.accept(b, id).await.unwrap();
    }

    async fn invite(&self, from: Uuid, to: Uuid) -> Result<(), AppError> {
        let item = self
            .items
            .create(
                from,
                NewItem {
                    kind: "task".into(),
                    title: "Ortak".into(),
                    description: None,
                },
            )
            .await
            .unwrap()
            .id;
        self.items
            .invite(
                from,
                item,
                CreateInvitationRequest {
                    to_user: to,
                    role: InvitationRole::Watcher,
                },
            )
            .await
            .map(|_| ())
    }
}

#[tokio::test]
async fn ak1_request_accept_and_unfriend() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;

    let sent = w.social.send(ada, "  BOB@Example.com ").await.unwrap();
    assert_eq!(sent.to_email, "bob@example.com");
    assert_eq!(
        w.social.outgoing(ada).await.unwrap(),
        std::slice::from_ref(&sent)
    );
    let incoming = w.social.incoming(bob).await.unwrap();
    assert_eq!(incoming.len(), 1);
    assert_eq!(
        (incoming[0].from_user, incoming[0].from_name.as_str()),
        (ada, "Ada")
    );
    // Only the recipient answers.
    assert!(matches!(
        w.social.accept(ada, sent.id).await,
        Err(AppError::NotFound)
    ));

    let friend = w.social.accept(bob, sent.id).await.unwrap();
    assert_eq!((friend.user_id, friend.display_name.as_str()), (ada, "Ada"));
    assert_eq!(w.social.friends(ada).await.unwrap()[0].user_id, bob);
    assert_eq!(w.social.friends(bob).await.unwrap()[0].user_id, ada);
    assert!(w.social.outgoing(ada).await.unwrap().is_empty());
    assert!(matches!(
        w.social.send(ada, "bob@example.com").await,
        Err(AppError::Conflict(_))
    ));

    w.social.unfriend(bob, ada).await.unwrap();
    assert!(w.social.friends(ada).await.unwrap().is_empty());
    assert!(matches!(
        w.social.unfriend(bob, ada).await,
        Err(AppError::NotFound)
    ));
}

#[tokio::test]
async fn ak1_decline_and_cancel() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let cem = w.user("cem@example.com", "Cem").await;

    let to_bob = w.social.send(ada, "bob@example.com").await.unwrap();
    w.social.decline(bob, to_bob.id).await.unwrap();
    assert!(w.social.incoming(bob).await.unwrap().is_empty());
    assert!(w.social.friends(ada).await.unwrap().is_empty());

    let to_cem = w.social.send(ada, "cem@example.com").await.unwrap();
    // Nobody else takes back or answers Ada's request.
    assert!(matches!(
        w.social.cancel(bob, to_cem.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        w.social.decline(bob, to_cem.id).await,
        Err(AppError::NotFound)
    ));
    w.social.cancel(ada, to_cem.id).await.unwrap();
    assert!(w.social.incoming(cem).await.unwrap().is_empty());
    assert!(matches!(
        w.social.accept(cem, to_cem.id).await,
        Err(AppError::NotFound)
    ));
    // Cancelled, not declined: asking again is fine.
    w.social.send(ada, "cem@example.com").await.unwrap();
}

#[tokio::test]
async fn ak2_a_declined_request_waits_and_is_not_told_apart() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let first = w.social.send(ada, "bob@example.com").await.unwrap();
    let duplicate = w.social.send(ada, "bob@example.com").await;
    w.social.decline(bob, first.id).await.unwrap();
    let too_soon = w.social.send(ada, "bob@example.com").await;
    // The same answer for "already pending" and "declined lately".
    let message = |result: Result<_, AppError>| match result {
        Err(AppError::Conflict(message)) => message,
        other => panic!("expected a conflict, got {other:?}"),
    };
    assert_eq!(message(duplicate), message(too_soon));
    w.clock.advance(DECLINE_COOLDOWN + Duration::minutes(1));
    w.social.send(ada, "bob@example.com").await.unwrap();
    assert_eq!(w.social.incoming(bob).await.unwrap().len(), 1);
}

#[tokio::test]
async fn ak2_requests_are_rate_limited() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    for n in 0..REQUESTS_PER_DAY {
        w.social
            .send(ada, &format!("kisi{n}@example.com"))
            .await
            .unwrap();
    }
    assert!(matches!(
        w.social.send(ada, "bir-fazla@example.com").await,
        Err(AppError::RateLimited { .. })
    ));
    w.clock.advance(Duration::hours(24));
    w.social.send(ada, "bir-fazla@example.com").await.unwrap();
}

#[tokio::test]
async fn ak2_a_blocked_sender_learns_nothing_and_reaches_nobody() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.social.block(bob, ada).await.unwrap();

    // To Bob, who blocked her, and to an address with no account: the same answer.
    let to_bob = w.social.send(ada, "bob@example.com").await.unwrap();
    let to_nobody = w.social.send(ada, "kimse@example.com").await.unwrap();
    assert_eq!(to_bob.to_email, "bob@example.com");
    assert_eq!(to_nobody.to_email, "kimse@example.com");
    assert_eq!(w.social.outgoing(ada).await.unwrap().len(), 2);
    assert!(w.social.incoming(bob).await.unwrap().is_empty());
    assert!(matches!(
        w.social.accept(bob, to_bob.id).await,
        Err(AppError::NotFound)
    ));
    // Nor can she share with him.
    assert!(!w.social.may_share(ada, bob).await.unwrap());
}

#[tokio::test]
async fn ak1_blocking_ends_the_friendship_and_open_requests() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    w.befriend(ada, bob, "bob@example.com").await;
    w.social.send(bob, "x@example.com").await.unwrap();
    let pending = w.social.send(ada, "cem@example.com").await.unwrap();

    w.social.block(ada, bob).await.unwrap();
    assert!(w.social.friends(ada).await.unwrap().is_empty());
    assert!(w.social.friends(bob).await.unwrap().is_empty());
    assert_eq!(w.social.blocked(ada).await.unwrap()[0].user_id, bob);
    // Ada cannot ask Bob while she blocks him; her other requests are untouched.
    assert!(matches!(
        w.social.send(ada, "bob@example.com").await,
        Err(AppError::Validation(_))
    ));
    assert_eq!(w.social.outgoing(ada).await.unwrap(), [pending]);
    // Blocking oneself is refused; an unknown id changes nothing.
    assert!(matches!(
        w.social.block(ada, ada).await,
        Err(AppError::Validation(_))
    ));
    w.social.block(ada, Uuid::now_v7()).await.unwrap();
    assert_eq!(w.social.blocked(ada).await.unwrap().len(), 1);

    w.social.unblock(ada, bob).await.unwrap();
    assert!(w.social.blocked(ada).await.unwrap().is_empty());
    assert!(matches!(
        w.social.unblock(ada, bob).await,
        Err(AppError::NotFound)
    ));
    w.social.send(ada, "bob@example.com").await.unwrap();
}

#[tokio::test]
async fn v1_only_friends_who_did_not_block_each_other_share() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    let bob = w.user("bob@example.com", "Bob").await;
    let cem = w.user("cem@example.com", "Cem").await;
    assert!(w.invite(ada, bob).await.is_err(), "strangers do not share");
    w.befriend(ada, bob, "bob@example.com").await;
    w.invite(ada, bob).await.unwrap();
    assert!(w.invite(ada, cem).await.is_err());
    w.social.block(bob, ada).await.unwrap();
    assert!(w.invite(ada, bob).await.is_err(), "a block ends sharing");
    assert!(!w.social.may_share(ada, ada).await.unwrap());
}

#[tokio::test]
async fn requests_are_checked() {
    let w = World::new().await;
    let ada = w.user("ada@example.com", "Ada").await;
    assert!(matches!(
        w.social.send(ada, "ada").await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        w.social.send(ada, "ADA@example.com").await,
        Err(AppError::Validation(_))
    ));
}
