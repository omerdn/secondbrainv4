//! `V3` Access inherited through a link is view + edit only: never share, link, manage or delete.

use access::{Perms, Relation as R};
use items::{CreateInvitationRequest, InvitationRole, ItemChanges};
use kernel::AppError;

use crate::support::{World, id, t};

/// Ayşe sees task T; Ali's page P is linked to T. Ayşe can view and edit P, but sharing it,
/// linking it elsewhere, managing its members and deleting it are all forbidden.
#[tokio::test]
async fn link_inheritance_grants_view_and_edit_only() {
    let world = World::new().await;
    let (ali, ayse, can) = (id(), id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let task = world.item(ali, "task", "T").await;
    let elsewhere = world.item(ayse, "task", "Ayşe's").await;
    world.grant(&[t(task.id, R::Watcher, ayse)]).await;
    items.link(ali, page.id, task.id).await.unwrap();

    // View and edit.
    assert_eq!(items.get(ayse, page.id).await.unwrap().id, page.id);
    let edit = ItemChanges {
        title: Some("P, edited".into()),
        description: None,
    };
    items.update(ayse, page.id, edit).await.unwrap();

    // Never share, link, manage members or delete (V3).
    let permissions = world.permissions();
    for perm in [Perms::SHARE, Perms::LINK, Perms::MANAGE, Perms::DELETE] {
        assert!(
            !permissions
                .check(world.pool(), ayse, page.id, perm)
                .await
                .unwrap()
        );
    }
    let invite = items
        .invite(
            ayse,
            page.id,
            CreateInvitationRequest {
                to_user: can,
                role: InvitationRole::Viewer,
            },
        )
        .await;
    assert!(matches!(invite, Err(AppError::Forbidden)));
    assert!(matches!(
        items.link(ayse, page.id, elsewhere.id).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        items.remove_member(ayse, page.id, ali).await,
        Err(AppError::Forbidden | AppError::Validation(_))
    ));
    assert!(matches!(
        items.set_trashed(ayse, page.id, true).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        items.set_archived(ayse, page.id, true).await,
        Err(AppError::Forbidden)
    ));
}
