//! `V6` Only the owner may delete a page or an item.

use access::Relation as R;
use items::{ItemChanges, NewItem};
use kernel::AppError;

use crate::support::{World, id, pending, t};

/// Ayşe is an assignee of Ali's task and can edit it, but deleting it is forbidden; Ali can.
#[tokio::test]
async fn only_the_owner_deletes_an_item() {
    let world = World::new().await;
    let items = world.items();
    let (ali, ayse) = (id(), id());
    let task = items
        .create(
            ali,
            NewItem {
                kind: "task".into(),
                title: "Plan".into(),
                description: None,
            },
        )
        .await
        .unwrap();
    world.grant(&[t(task.id, R::Assignee, ayse)]).await;

    let edit = ItemChanges {
        title: Some("Plan the trip".into()),
        description: None,
    };
    items.update(ayse, task.id, edit).await.unwrap();
    assert!(matches!(
        items.set_trashed(ayse, task.id, true).await,
        Err(AppError::Forbidden)
    ));
    assert!(
        items
            .set_trashed(ali, task.id, true)
            .await
            .unwrap()
            .deleted_at
            .is_some()
    );
}

/// Ayşe edits Ali's page through a link; deleting the page is forbidden.
#[tokio::test]
#[ignore = "T3.2.2: inherited visibility end to end"]
async fn link_editors_cannot_delete_a_page() {
    pending()
}
