//! `V8` Access is recomputed on every relevant change; access that came through a removed link
//! drops automatically.

use access::Relation as R;
use kernel::AppError;

use crate::support::{World, id, pending, t};

/// Ayşe sees page P only through its link to task T. After Ali unlinks P from T she can no longer
/// see P.
#[tokio::test]
async fn unlinking_drops_inherited_access() {
    let world = World::new().await;
    let (ali, ayse) = (id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let task = world.item(ali, "task", "T").await;
    world.grant(&[t(task.id, R::Watcher, ayse)]).await;
    items.link(ali, page.id, task.id).await.unwrap();
    assert_eq!(items.get(ayse, page.id).await.unwrap().id, page.id);

    items.unlink(ali, page.id, task.id).await.unwrap();

    assert!(matches!(
        items.get(ayse, page.id).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(items.get(ayse, task.id).await.unwrap().id, task.id);
}

/// Removing Ayşe from task T removes her access to T and to the pages linked to it.
#[tokio::test]
#[ignore = "T3.2.1: item member invitations"]
async fn removing_a_member_drops_access() {
    pending()
}

/// Removing Ayşe from Ali's notebook removes her access to its pages.
#[tokio::test]
#[ignore = "T3.4.1: notebook membership API"]
async fn notebook_membership_change_recomputes() {
    pending()
}

/// Demoting Mehmet from manager to member in team X removes his access to assignee-only team items.
#[tokio::test]
#[ignore = "T5.1.2: team roles and permission matrix"]
async fn team_role_change_recomputes() {
    pending()
}

/// Switching a team task from "all team" to "assignees only" removes access for other members.
#[tokio::test]
#[ignore = "T5.1.3: team items and visibility mode"]
async fn visibility_mode_change_recomputes() {
    pending()
}
