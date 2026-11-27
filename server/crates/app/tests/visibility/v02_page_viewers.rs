//! `V2` Page viewers = notebook members ∪ viewers of *every* item the page is linked to.

use access::Relation as R;
use kernel::AppError;

use crate::support::{World, id, pending, t};

/// Page P is in Ali's notebook and linked to tasks T1 (shared with Ayşe) and T2 (shared with Can).
/// Ali, Ayşe and Can can all view P; Deniz, who sees neither task, cannot.
#[tokio::test]
async fn viewers_of_each_linked_item_see_the_page() {
    let world = World::new().await;
    let (ali, ayse, can, deniz) = (id(), id(), id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let first = world.item(ali, "task", "T1").await;
    let second = world.item(ali, "task", "T2").await;
    world
        .grant(&[
            t(first.id, R::Watcher, ayse),
            t(second.id, R::Assignee, can),
        ])
        .await;
    items.link(ali, page.id, first.id).await.unwrap();
    items.link(ali, page.id, second.id).await.unwrap();

    for user in [ali, ayse, can] {
        assert_eq!(items.get(user, page.id).await.unwrap().id, page.id);
    }
    assert!(matches!(
        items.get(deniz, page.id).await,
        Err(AppError::NotFound)
    ));
    // Ayşe sees the page through T1 only: she still does not see T2.
    assert!(matches!(
        items.get(ayse, second.id).await,
        Err(AppError::NotFound)
    ));
}

/// Ali adds Ayşe to his notebook as a viewer. She sees every page in it, linked or not.
#[tokio::test]
#[ignore = "T3.4.1: notebook membership API"]
async fn notebook_members_see_its_pages() {
    pending()
}

/// Inheritance works the same for every kind a page links to: a viewer of a routine, a tracker or
/// a goal sees its linked page, can edit it, and loses it when unlinked (V2, V3, V8).
#[tokio::test]
async fn every_kind_passes_its_viewers_to_linked_pages() {
    let world = World::new().await;
    let items = world.items();
    // Every kind a page may link to (B1); notebook membership is T3.4.1.
    for kind in ["task", "routine", "tracker", "goal"] {
        let (ali, ayse) = (id(), id());
        let page = world.item(ali, "page", "P").await;
        let item = world.item(ali, kind, kind).await;
        world.grant(&[t(item.id, R::Watcher, ayse)]).await;
        assert!(
            matches!(items.get(ayse, page.id).await, Err(AppError::NotFound)),
            "{kind}"
        );
        items.link(ali, page.id, item.id).await.unwrap();
        assert_eq!(
            items.get(ayse, page.id).await.unwrap().id,
            page.id,
            "{kind}"
        );
        items.unlink(ali, page.id, item.id).await.unwrap();
        assert!(
            matches!(items.get(ayse, page.id).await, Err(AppError::NotFound)),
            "{kind}"
        );
    }
}
