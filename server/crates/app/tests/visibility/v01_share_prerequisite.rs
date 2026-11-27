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

/// Ali and Mehmet are in the same team but not friends. Ali can share a personal task with him.
#[tokio::test]
#[ignore = "T5.1.1: teams and membership"]
async fn sharing_with_a_teammate_is_allowed() {
    pending()
}
