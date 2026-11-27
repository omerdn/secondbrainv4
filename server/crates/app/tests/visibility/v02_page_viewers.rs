//! `V2` Page viewers = notebook members ∪ viewers of *every* item the page is linked to.

use access::{Perms, Relation as R};
use kernel::AppError;

use crate::support::{World, id, t};

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

/// Ali adds Ayşe to his notebook as a viewer. She sees every page in it, linked or not, but cannot
/// write in it; Can joins as an editor and can (Y2). Deniz, not a member, sees none of it.
#[tokio::test]
async fn notebook_members_see_its_pages() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayşe").await;
    let (can, can_email) = world.person("Can").await;
    let deniz = id();
    world.befriend(ali, ayse, &ayse_email).await;
    world.befriend(ali, can, &can_email).await;
    let (notebook, page) = world
        .shared_notebook(ali, ayse, items::InvitationRole::Viewer)
        .await;
    let notebooks = world.notebooks();
    let later = notebooks
        .create_page(
            ali,
            notebook,
            notebooks::CreatePageRequest { title: "Q".into() },
        )
        .await
        .unwrap()
        .item
        .id;
    let items = world.items();
    let perms = world.permissions();
    for p in [page, later] {
        assert_eq!(items.get(ayse, p).await.unwrap().id, p);
        assert!(
            !perms
                .check(world.pool(), ayse, p, Perms::EDIT)
                .await
                .unwrap()
        );
        assert!(matches!(items.get(deniz, p).await, Err(AppError::NotFound)));
    }
    assert!(matches!(
        notebooks
            .create_page(
                ayse,
                notebook,
                notebooks::CreatePageRequest { title: "x".into() }
            )
            .await,
        Err(AppError::Forbidden)
    ));

    let invitation = world
        .sharing_items()
        .invite(
            ali,
            notebook,
            items::CreateInvitationRequest {
                to_user: can,
                role: items::InvitationRole::Editor,
            },
        )
        .await
        .unwrap()
        .id;
    world
        .sharing_items()
        .accept_invitation(can, invitation)
        .await
        .unwrap();
    assert!(
        perms
            .check(world.pool(), can, page, Perms::EDIT)
            .await
            .unwrap()
    );
    let his = notebooks
        .create_page(
            can,
            notebook,
            notebooks::CreatePageRequest { title: "C".into() },
        )
        .await
        .unwrap();
    // Y5: a page belongs to the notebook's owner, whoever writes it.
    assert_eq!(his.item.owner, items::Owner::User { user_id: ali });
    // A notebook takes editors and viewers only.
    assert!(matches!(
        world
            .sharing_items()
            .invite(
                ali,
                notebook,
                items::CreateInvitationRequest {
                    to_user: can,
                    role: items::InvitationRole::Assignee,
                },
            )
            .await,
        Err(AppError::Validation(_))
    ));
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
