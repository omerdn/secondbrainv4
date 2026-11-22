//! `V4` Only the page owner may link a page to an item, and only to an item they can view. On team
//! pages the right can be given to authorised roles.

use access::{Perms, Relation as R};
use kernel::AppError;

use crate::support::{World, id, pending, t};

/// Ayşe can edit Ali's page through a link but does not own it: linking it to her own task fails.
#[tokio::test]
async fn non_owner_cannot_link_a_page() {
    let world = World::new().await;
    let (ali, ayse) = (id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let alis_task = world.item(ali, "task", "T").await;
    let ayses_task = world.item(ayse, "task", "Hers").await;
    world.grant(&[t(alis_task.id, R::Assignee, ayse)]).await;
    items.link(ali, page.id, alis_task.id).await.unwrap();
    assert!(
        world
            .permissions()
            .check(world.pool(), ayse, page.id, Perms::VIEW | Perms::EDIT)
            .await
            .unwrap(),
        "Ayşe can edit the page through the link"
    );

    assert!(matches!(
        items.link(ayse, page.id, ayses_task.id).await,
        Err(AppError::Forbidden)
    ));
    assert_eq!(items.links(ali, page.id).await.unwrap().len(), 1);
}

/// Ali owns page P but cannot see Ayşe's private task T: linking P to T fails and reveals nothing
/// about T.
#[tokio::test]
async fn owner_cannot_link_to_an_item_they_cannot_see() {
    let world = World::new().await;
    let (ali, ayse) = (id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let private = world.item(ayse, "task", "Private").await;

    // The same answer as for an id that does not exist.
    assert!(matches!(
        items.link(ali, page.id, private.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        items.link(ali, page.id, id()).await,
        Err(AppError::NotFound)
    ));
    assert!(items.links(ali, page.id).await.unwrap().is_empty());
    assert!(
        !world
            .permissions()
            .check(world.pool(), ayse, page.id, Perms::VIEW)
            .await
            .unwrap(),
        "the failed link gave Ayşe nothing"
    );
}

/// In team X the manager role is allowed to link team pages; a manager links one, a member cannot.
#[tokio::test]
#[ignore = "T5.1.5: team notebooks"]
async fn team_roles_can_be_allowed_to_link_team_pages() {
    pending()
}
