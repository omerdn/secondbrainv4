//! `V10` When a user loses access, their open sessions on that object are cut immediately.

use access::Relation as R;
use db::outbox::{DispatchConfig, Dispatcher};
use identity::{SessionId, UserId};
use realtime::{Bridge, Channel, Event, Hub, Notifier, ServerMessage, UnsubscribeReason};

use crate::support::{World, id, t};

/// Ayşe has page P open over the realtime channel. Ali removes her access; her subscription to P
/// is terminated and further updates are not delivered. (The WebSocket round trip itself is
/// covered in `realtime/tests/access_revoked.rs`.)
#[tokio::test]
async fn revoked_user_is_disconnected() {
    let world = World::new().await;
    let (ali, ayse, page, task) = (id(), id(), id(), id());
    world
        .grant(&[
            t(page, R::Owner, ali),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(page, R::LinkedTo, task),
        ])
        .await;

    let hub = Hub::new();
    let bridge = Bridge::start(world.pool(), world.schema(), hub.clone())
        .await
        .unwrap();
    let mut dispatcher = Dispatcher::new(DispatchConfig::default());
    realtime::access::register(
        &mut dispatcher,
        world.pool().clone(),
        Notifier::new(world.schema()),
    );
    let mut connection = hub.connect(UserId::from_uuid(ayse), SessionId::new());
    hub.subscribe(connection.id, Channel::Item(page));

    // Unlinking the page from the task takes Ayşe's only path to it away.
    world.revoke(t(page, R::LinkedTo, task)).await;
    dispatcher.dispatch_batch(world.pool()).await.unwrap();

    let message = tokio::time::timeout(std::time::Duration::from_secs(5), connection.outbox.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message,
        ServerMessage::Unsubscribed {
            channel: Channel::Item(page).to_string(),
            reason: Some(UnsubscribeReason::AccessRevoked),
        }
    );
    let update = Event {
        name: "item.changed".into(),
        data: serde_json::json!({}),
    };
    assert_eq!(hub.publish(&Channel::Item(page), &update), 0);
    bridge.stop().await;
}

/// Ayşe has a page of Ali's notebook open as a member. Ali removes her from the notebook; her
/// subscription to the page is cut as well (V8, T3.4.1).
#[tokio::test]
async fn removing_a_notebook_member_disconnects_them_from_its_pages() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayse").await;
    world.befriend(ali, ayse, &ayse_email).await;
    let (notebook, page) = world
        .shared_notebook(ali, ayse, items::InvitationRole::Viewer)
        .await;

    let hub = Hub::new();
    let bridge = Bridge::start(world.pool(), world.schema(), hub.clone())
        .await
        .unwrap();
    let mut dispatcher = Dispatcher::new(DispatchConfig::default());
    realtime::access::register(
        &mut dispatcher,
        world.pool().clone(),
        Notifier::new(world.schema()),
    );
    // Events from setting the notebook up are not about Ayşe losing anything.
    dispatcher.dispatch_batch(world.pool()).await.unwrap();
    let mut connection = hub.connect(UserId::from_uuid(ayse), SessionId::new());
    hub.subscribe(connection.id, Channel::Item(page));

    world
        .sharing_items()
        .remove_member(ali, notebook, ayse)
        .await
        .unwrap();
    dispatcher.dispatch_batch(world.pool()).await.unwrap();

    let message = tokio::time::timeout(std::time::Duration::from_secs(5), connection.outbox.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message,
        ServerMessage::Unsubscribed {
            channel: Channel::Item(page).to_string(),
            reason: Some(UnsubscribeReason::AccessRevoked),
        }
    );
    bridge.stop().await;
}

/// Deniz has a team task open; he leaves the team and his subscription ends at once (TK7,
/// T5.1.6).
#[tokio::test]
async fn leaving_a_team_disconnects() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (deniz, _) = world.person("Deniz").await;
    let team = world.team(admin, &[(deniz, teams::TeamRole::Member)]).await;
    let item = world
        .team_task(team, admin, items::TeamVisibilityMode::All)
        .await;

    let hub = Hub::new();
    let bridge = Bridge::start(world.pool(), world.schema(), hub.clone())
        .await
        .unwrap();
    let mut dispatcher = Dispatcher::new(DispatchConfig::default());
    realtime::access::register(
        &mut dispatcher,
        world.pool().clone(),
        Notifier::new(world.schema()),
    );
    let mut connection = hub.connect(UserId::from_uuid(deniz), SessionId::new());
    hub.subscribe(connection.id, Channel::Item(item));

    world.teams().leave(deniz, team).await.unwrap();
    dispatcher.dispatch_batch(world.pool()).await.unwrap();

    let message = tokio::time::timeout(std::time::Duration::from_secs(5), connection.outbox.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message,
        ServerMessage::Unsubscribed {
            channel: Channel::Item(item).to_string(),
            reason: Some(UnsubscribeReason::AccessRevoked),
        }
    );
    bridge.stop().await;
}
