//! `V4` Only the page owner may link a page to an item, and only to an item they can view. On team
//! pages the right can be given to authorised roles.

use access::{Perms, Relation as R};
use kernel::AppError;

use crate::support::{World, id, t};

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
/// Taking the right away from the role takes it from the manager too (T5.1.5).
#[tokio::test]
async fn team_roles_can_be_allowed_to_link_team_pages() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (mehmet, _) = world.person("Mehmet").await;
    let (zeynep, _) = world.person("Zeynep").await;
    let team = world
        .team(
            admin,
            &[
                (mehmet, teams::TeamRole::Manager),
                (zeynep, teams::TeamRole::Member),
            ],
        )
        .await;
    let notebooks = world.notebooks();
    let notebook = notebooks
        .create_for_team(
            admin,
            team,
            notebooks::CreateTeamNotebookRequest {
                title: "Takım notları".into(),
                description: None,
                visibility: None,
                editors: None,
                viewers: None,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    // A member writes the page; it belongs to the team (Y5) and she can edit it, not link it.
    let page = notebooks
        .create_page(
            zeynep,
            notebook,
            notebooks::CreatePageRequest {
                title: "Toplantı".into(),
            },
        )
        .await
        .unwrap()
        .item
        .id;
    let task = world
        .team_task(team, admin, items::TeamVisibilityMode::All)
        .await;
    let items = world.items();
    assert!(matches!(
        items.link(zeynep, page, task).await,
        Err(AppError::Forbidden)
    ));

    // The admin takes "link" away from managers: Mehmet cannot link either.
    let teams = world.teams();
    let mut matrix = teams.matrix(admin, team).await.unwrap();
    for row in &mut matrix.roles {
        if row.role == teams::TeamRole::Manager {
            row.item = vec![teams::ItemPermission::View, teams::ItemPermission::Edit];
        }
    }
    teams.set_matrix(admin, team, matrix.clone()).await.unwrap();
    assert!(matches!(
        items.link(mehmet, page, task).await,
        Err(AppError::Forbidden)
    ));

    // Allowed again: the manager links the team page.
    for row in &mut matrix.roles {
        if row.role == teams::TeamRole::Manager {
            row.item.push(teams::ItemPermission::Link);
        }
    }
    teams.set_matrix(admin, team, matrix).await.unwrap();
    items.link(mehmet, page, task).await.unwrap();
    assert_eq!(items.links(mehmet, page).await.unwrap().len(), 1);
    assert!(
        world
            .permissions()
            .check(world.pool(), zeynep, page, Perms::EDIT)
            .await
            .unwrap(),
        "the member still edits the page"
    );
}
