//! `V7` Before any action that widens visibility, the users who would gain access are listed.

use access::{Perms, Relation as R};

use crate::support::{World, id, t};

/// Task T is shared with Ayşe and Mehmet. Previewing "link page P to T" lists exactly Ayşe and
/// Mehmet, and changes nothing.
#[tokio::test]
async fn linking_preview_lists_new_viewers() {
    let world = World::new().await;
    let (ali, ayse, mehmet, page, task) = (id(), id(), id(), id(), id());
    world
        .grant(&[
            t(page, R::Owner, ali),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(task, R::Watcher, mehmet),
        ])
        .await;
    let mut conn = world.pool().acquire().await.unwrap();
    let preview = world
        .permissions()
        .preview(&mut conn, t(page, R::LinkedTo, task))
        .await
        .unwrap();
    let mut expected = vec![ayse, mehmet];
    expected.sort();
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].item_id, page);
    assert_eq!(preview[0].new_viewers, expected);
    // Nothing changed.
    assert!(
        !world
            .permissions()
            .check(world.pool(), ayse, page, Perms::VIEW)
            .await
            .unwrap()
    );
}

/// Task T has two linked pages. Previewing "add Can to T" reports Can gaining T and both pages.
#[tokio::test]
async fn adding_a_member_previews_linked_pages() {
    let world = World::new().await;
    let (ali, ayse, can) = (id(), id(), id());
    let items = world.items();
    let task = world.item(ali, "task", "T").await;
    let first = world.item(ali, "page", "P1").await;
    let second = world.item(ali, "page", "P2").await;
    world.grant(&[t(task.id, R::Watcher, ayse)]).await;
    items.link(ali, first.id, task.id).await.unwrap();
    items.link(ali, second.id, task.id).await.unwrap();

    let mut conn = world.pool().acquire().await.unwrap();
    let preview = world
        .permissions()
        .preview(&mut conn, t(task.id, R::Watcher, can))
        .await
        .unwrap();
    let mut gained: Vec<_> = preview.iter().map(|item| item.item_id).collect();
    gained.sort();
    let mut expected = vec![task.id, first.id, second.id];
    expected.sort();
    assert_eq!(gained, expected);
    // Only Can gains anything: Ayşe already sees all three.
    assert!(preview.iter().all(|item| item.new_viewers == [can]));
    // A preview changes nothing.
    assert!(
        !world
            .permissions()
            .check(world.pool(), can, first.id, Perms::VIEW)
            .await
            .unwrap()
    );
}
