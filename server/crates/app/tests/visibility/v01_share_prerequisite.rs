//! `V1` Sharing prerequisite: an item can only be shared with a friend or a teammate.

use items::{CreateInvitationRequest, InvitationRole};
use kernel::AppError;

use crate::support::{World, pending};

fn watcher(to_user: uuid::Uuid) -> CreateInvitationRequest {
    CreateInvitationRequest {
        to_user,
        role: InvitationRole::Watcher,
    }
}

/// Ali owns a task; Ayşe is neither his friend nor in any of his teams. Inviting her is rejected
/// and no relation is created.
#[tokio::test]
async fn sharing_with_a_stranger_is_rejected() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, _) = world.person("Ayse").await;
    let items = world.sharing_items();
    let task = world.item(ali, "task", "T").await;

    assert!(matches!(
        items.invite(ali, task.id, watcher(ayse)).await,
        Err(AppError::Forbidden | AppError::Validation(_))
    ));
    assert!(items.pending_invitations(ayse).await.unwrap().is_empty());
    assert!(matches!(
        items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
}

/// Ali and Ayşe are friends. Inviting Ayşe succeeds; after she accepts she can view the task.
#[tokio::test]
async fn sharing_with_a_friend_is_allowed() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayse").await;
    world.befriend(ali, ayse, &ayse_email).await;
    let items = world.sharing_items();
    let task = world.item(ali, "task", "T").await;

    let invitation = items.invite(ali, task.id, watcher(ayse)).await.unwrap();
    // Not before she accepts (G5).
    assert!(matches!(
        items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
    items.accept_invitation(ayse, invitation.id).await.unwrap();
    assert_eq!(items.get(ayse, task.id).await.unwrap().id, task.id);
}

/// Ali and Ayşe were friends. Once the friendship ends Ali can no longer invite her, an invitation
/// sent before cannot be accepted, and what was already shared stays shared (V8). Once Ayşe blocks
/// him he still cannot invite her (AK2).
#[tokio::test]
async fn ending_a_friendship_or_blocking_stops_new_shares() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayse").await;
    world.befriend(ali, ayse, &ayse_email).await;
    let items = world.sharing_items();
    let shared = world.item(ali, "task", "Shared").await;
    let invitation = items.invite(ali, shared.id, watcher(ayse)).await.unwrap();
    items.accept_invitation(ayse, invitation.id).await.unwrap();
    let pending = world.item(ali, "task", "Pending").await;
    let open = items.invite(ali, pending.id, watcher(ayse)).await.unwrap();

    world.friendships().unfriend(ali, ayse).await.unwrap();

    let later = world.item(ali, "task", "Later").await;
    assert!(matches!(
        items.invite(ali, later.id, watcher(ayse)).await,
        Err(AppError::Forbidden | AppError::Validation(_))
    ));
    assert!(items.accept_invitation(ayse, open.id).await.is_err());
    assert!(matches!(
        items.get(ayse, pending.id).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(items.get(ayse, shared.id).await.unwrap().id, shared.id);

    world.friendships().block(ayse, ali).await.unwrap();
    assert!(matches!(
        items.invite(ali, later.id, watcher(ayse)).await,
        Err(AppError::Forbidden | AppError::Validation(_))
    ));
    assert!(items.pending_invitations(ayse).await.unwrap().is_empty());
}

/// Ali and Mehmet are in the same team but not friends. Ali can share a personal task with him.
#[tokio::test]
#[ignore = "T5.1.1: teams and membership"]
async fn sharing_with_a_teammate_is_allowed() {
    pending()
}
