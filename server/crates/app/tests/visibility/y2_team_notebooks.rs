//! `Y2` Team notebooks (T5.1.5): owned by a team; visible to the whole team or to the chosen
//! people (and managers and above), chosen per notebook. Their pages belong to the team (Y5) and
//! are seen through the notebook (V2).

use items::TeamVisibilityMode;
use kernel::AppError;
use notebooks::{CreatePageRequest, CreateTeamNotebookRequest};
use teams::TeamRole;
use uuid::Uuid;

use crate::support::World;

fn request(
    visibility: TeamVisibilityMode,
    editors: &[Uuid],
    viewers: &[Uuid],
) -> CreateTeamNotebookRequest {
    CreateTeamNotebookRequest {
        title: "Takım defteri".into(),
        description: None,
        visibility: Some(visibility),
        editors: Some(editors.to_vec()),
        viewers: Some(viewers.to_vec()),
    }
}

fn page(title: &str) -> CreatePageRequest {
    CreatePageRequest {
        title: title.into(),
    }
}

async fn sees(world: &World, user: Uuid, item: Uuid) -> bool {
    match world.items().get(user, item).await {
        Ok(found) => found.id == item,
        Err(AppError::NotFound) => false,
        Err(err) => panic!("{err}"),
    }
}

/// A team of an admin, a manager, three members and a guest.
async fn team_of_six(world: &World) -> (Uuid, [Uuid; 6]) {
    let mut people = [Uuid::nil(); 6];
    for (slot, name) in people
        .iter_mut()
        .zip(["Admin", "Mehmet", "Zeynep", "Deniz", "Can", "Misafir"])
    {
        *slot = world.person(name).await.0;
    }
    let [admin, manager, a, b, c, guest] = people;
    let team = world
        .team(
            admin,
            &[
                (manager, TeamRole::Manager),
                (a, TeamRole::Member),
                (b, TeamRole::Member),
                (c, TeamRole::Member),
                (guest, TeamRole::Guest),
            ],
        )
        .await;
    (team, people)
}

/// "All team": every member sees the notebook and the pages written in it; outsiders see nothing.
#[tokio::test]
async fn all_team_notebook_is_the_teams() {
    let world = World::new().await;
    let (team, people) = team_of_six(&world).await;
    let [_, _, writer, other, _, guest] = people;
    let (outsider, _) = world.person("Dışarıdan").await;
    let notebooks = world.notebooks();
    let notebook = notebooks
        .create_for_team(writer, team, request(TeamVisibilityMode::All, &[], &[]))
        .await
        .unwrap();
    assert_eq!(notebook.item.owner, items::Owner::Team { team_id: team });
    let written = notebooks
        .create_page(writer, notebook.item.id, page("Toplantı"))
        .await
        .unwrap();
    // Y5: the page belongs to the team, not to whoever wrote it.
    assert_eq!(written.item.owner, items::Owner::Team { team_id: team });

    for person in people {
        assert!(sees(&world, person, notebook.item.id).await, "{person}");
        assert!(sees(&world, person, written.item.id).await, "{person}");
    }
    assert!(!sees(&world, outsider, notebook.item.id).await);
    assert!(!sees(&world, outsider, written.item.id).await);
    // Members write in it; guests only read.
    notebooks
        .create_page(other, notebook.item.id, page("Notlar"))
        .await
        .unwrap();
    assert!(matches!(
        notebooks
            .create_page(guest, notebook.item.id, page("Misafir"))
            .await,
        Err(AppError::Forbidden)
    ));
    let listed = notebooks.notebooks(other).await.unwrap();
    assert!(listed.iter().any(|n| n.item.id == notebook.item.id));
    assert!(
        notebooks
            .notebooks(outsider)
            .await
            .unwrap()
            .iter()
            .all(|n| n.item.id != notebook.item.id)
    );
}

/// "Chosen people": the editors and viewers, and managers and above, see it; other members do not.
/// Switching to the whole team opens it, pages included (V8); switching back closes it, also for
/// someone who wrote a page in it while it was open.
#[tokio::test]
async fn chosen_people_notebook_limits_viewers() {
    let world = World::new().await;
    let (team, [admin, manager, editor, viewer, other, guest]) = team_of_six(&world).await;
    let notebooks = world.notebooks();
    let notebook = notebooks
        .create_for_team(
            admin,
            team,
            request(TeamVisibilityMode::Assignees, &[editor], &[viewer, guest]),
        )
        .await
        .unwrap()
        .item
        .id;
    let written = notebooks
        .create_page(editor, notebook, page("Plan"))
        .await
        .unwrap()
        .item
        .id;
    for person in [admin, manager, editor, viewer, guest] {
        assert!(sees(&world, person, notebook).await, "{person} sees it");
        assert!(
            sees(&world, person, written).await,
            "{person} sees the page"
        );
    }
    assert!(!sees(&world, other, notebook).await);
    assert!(!sees(&world, other, written).await);
    // A chosen viewer reads but does not write.
    assert!(matches!(
        notebooks.create_page(viewer, notebook, page("X")).await,
        Err(AppError::Forbidden)
    ));

    let items = world.items();
    items
        .set_team_visibility(admin, notebook, TeamVisibilityMode::All)
        .await
        .unwrap();
    assert!(sees(&world, other, written).await);
    let by_other = notebooks
        .create_page(other, notebook, page("Benim"))
        .await
        .unwrap()
        .item
        .id;
    items
        .set_team_visibility(admin, notebook, TeamVisibilityMode::Assignees)
        .await
        .unwrap();
    for item in [notebook, written, by_other] {
        assert!(!sees(&world, other, item).await, "{item} closed again");
    }
    assert!(sees(&world, manager, by_other).await);
}

/// Creating a team notebook follows the matrix ("create notebooks"); outsiders cannot tell the
/// team exists and only members can be chosen. Only roles with DELETE trash team pages (V6).
#[tokio::test]
async fn team_notebook_rights() {
    let world = World::new().await;
    let (team, [admin, manager, member, _, _, guest]) = team_of_six(&world).await;
    let (outsider, _) = world.person("Dışarıdan").await;
    let notebooks = world.notebooks();
    for team_id in [team, Uuid::now_v7()] {
        assert!(matches!(
            notebooks
                .create_for_team(
                    outsider,
                    team_id,
                    request(TeamVisibilityMode::All, &[], &[])
                )
                .await,
            Err(AppError::NotFound)
        ));
    }
    assert!(matches!(
        notebooks
            .create_for_team(guest, team, request(TeamVisibilityMode::All, &[], &[]))
            .await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        notebooks
            .create_for_team(
                admin,
                team,
                request(TeamVisibilityMode::Assignees, &[outsider], &[])
            )
            .await,
        Err(AppError::Validation(_))
    ));
    // The generic endpoint does not make team pages outside a notebook.
    let bare_page = items::NewTeamItem {
        item: items::CreateItemRequest {
            kind: "page".into(),
            title: "P".into(),
            description: None,
        }
        .validate()
        .unwrap(),
        visibility: TeamVisibilityMode::All,
        assignees: Vec::new(),
        watchers: Vec::new(),
        editors: Vec::new(),
        viewers: Vec::new(),
    };
    assert!(matches!(
        world.items().create_team_item(admin, team, bare_page).await,
        Err(AppError::Validation(_))
    ));

    let notebook = notebooks
        .create_for_team(member, team, request(TeamVisibilityMode::All, &[], &[]))
        .await
        .unwrap()
        .item
        .id;
    let written = notebooks
        .create_page(member, notebook, page("P"))
        .await
        .unwrap()
        .item
        .id;
    let items = world.items();
    for person in [member, manager] {
        assert!(matches!(
            items.set_trashed(person, written, true).await,
            Err(AppError::Forbidden)
        ));
    }
    items.set_trashed(admin, written, true).await.unwrap();
}
