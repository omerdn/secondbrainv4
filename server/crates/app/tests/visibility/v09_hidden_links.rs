//! `V9` A user who can see a page but not one of its linked items learns nothing about that link:
//! no title, no count, no placeholder.

use access::Relation as R;

use crate::support::{World, id, t};

/// Page P is linked to tasks T1 (visible to Ayşe) and T2 (not visible to her). Listing P's links as
/// Ayşe returns only T1, and no count or marker hints at T2.
#[tokio::test]
async fn invisible_link_targets_are_omitted_entirely() {
    let world = World::new().await;
    let (ali, ayse) = (id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let visible = world.item(ali, "task", "Visible").await;
    let hidden = world.item(ali, "task", "Secret title").await;
    items.link(ali, page.id, visible.id).await.unwrap();
    items.link(ali, page.id, hidden.id).await.unwrap();
    world
        .grant(&[
            t(page.id, R::Watcher, ayse),
            t(visible.id, R::Watcher, ayse),
        ])
        .await;

    let links = items.links(ayse, page.id).await.unwrap();
    assert_eq!(links.len(), 1, "the hidden link is not even counted");
    assert_eq!(links[0].item.id, visible.id);
    let body = serde_json::to_string(&links).unwrap();
    assert!(!body.contains("Secret"));
    assert!(!body.contains(&hidden.id.to_string()));
    assert_eq!(items.links(ali, page.id).await.unwrap().len(), 2);
}
