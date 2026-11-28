//! `V8` Access is recomputed on every relevant change; access that came through a removed link
//! drops automatically.

use access::Relation as R;
use kernel::AppError;

use crate::support::{World, id, t};

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
async fn removing_a_member_drops_access() {
    let world = World::new().await;
    let (ali, ayse) = (id(), id());
    let items = world.items();
    let page = world.item(ali, "page", "P").await;
    let task = world.item(ali, "task", "T").await;
    world.grant(&[t(task.id, R::Watcher, ayse)]).await;
    items.link(ali, page.id, task.id).await.unwrap();
    assert_eq!(items.get(ayse, page.id).await.unwrap().id, page.id);

    items.remove_member(ali, task.id, ayse).await.unwrap();

    assert!(matches!(
        items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        items.get(ayse, page.id).await,
        Err(AppError::NotFound)
    ));
    // The owner stays; removing someone who is not a member finds nothing.
    assert!(matches!(
        items.remove_member(ali, task.id, ali).await,
        Err(AppError::Validation(_))
    ));
    assert!(matches!(
        items.remove_member(ali, task.id, ayse).await,
        Err(AppError::NotFound)
    ));
}

/// Removing Ayşe from Ali's notebook removes her access to its pages (V8).
#[tokio::test]
async fn notebook_membership_change_recomputes() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayşe").await;
    world.befriend(ali, ayse, &ayse_email).await;
    let (notebook, page) = world
        .shared_notebook(ali, ayse, items::InvitationRole::Editor)
        .await;
    let items = world.items();
    assert_eq!(items.get(ayse, page).await.unwrap().id, page);

    items.remove_member(ali, notebook, ayse).await.unwrap();

    for item in [notebook, page] {
        assert!(matches!(
            items.get(ayse, item).await,
            Err(AppError::NotFound)
        ));
    }
    assert_eq!(items.get(ali, page).await.unwrap().id, page);
}

/// Demoting Mehmet from manager to member in team X removes his access to assignee-only team items.
#[tokio::test]
async fn team_role_change_recomputes() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (mehmet, _) = world.person("Mehmet").await;
    let team = world
        .team(admin, &[(mehmet, teams::TeamRole::Manager)])
        .await;
    let item = world
        .team_task(team, admin, items::TeamVisibilityMode::Assignees)
        .await;
    let items = world.items();
    assert_eq!(items.get(mehmet, item).await.unwrap().id, item);

    world
        .teams()
        .set_role(admin, team, mehmet, teams::TeamRole::Member)
        .await
        .unwrap();
    assert!(matches!(
        items.get(mehmet, item).await,
        Err(AppError::NotFound)
    ));
}

/// Switching a team task from "all team" to "assignees only" removes access for other members.
#[tokio::test]
async fn visibility_mode_change_recomputes() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (zeynep, _) = world.person("Zeynep").await;
    let (deniz, _) = world.person("Deniz").await;
    let team = world
        .team(
            admin,
            &[
                (zeynep, teams::TeamRole::Member),
                (deniz, teams::TeamRole::Member),
            ],
        )
        .await;
    let item = world
        .team_task_with(team, admin, items::TeamVisibilityMode::All, &[zeynep], &[])
        .await;
    let items = world.items();
    assert_eq!(items.get(deniz, item).await.unwrap().id, item);

    // A member without MANAGE cannot change the mode.
    assert!(matches!(
        items
            .set_team_visibility(zeynep, item, items::TeamVisibilityMode::Assignees)
            .await,
        Err(AppError::Forbidden)
    ));
    let changed = items
        .set_team_visibility(admin, item, items::TeamVisibilityMode::Assignees)
        .await
        .unwrap();
    assert_eq!(
        changed.team_visibility,
        Some(items::TeamVisibilityMode::Assignees)
    );
    assert!(matches!(
        items.get(deniz, item).await,
        Err(AppError::NotFound)
    ));
    assert_eq!(items.get(zeynep, item).await.unwrap().id, item);

    // And back: access widens again.
    items
        .set_team_visibility(admin, item, items::TeamVisibilityMode::All)
        .await
        .unwrap();
    assert_eq!(items.get(deniz, item).await.unwrap().id, item);

    // Personal items have no mode.
    let personal = world.item(admin, "task", "Kişisel").await;
    assert!(matches!(
        items
            .set_team_visibility(admin, personal.id, items::TeamVisibilityMode::All)
            .await,
        Err(AppError::Validation(_))
    ));
}

/// TK7: Zeynep leaves the team. Every team item she reached through it, as a member, an assignee
/// or a chosen notebook editor, is gone for her at once; the items stay in the team. Joining
/// again brings back only what her role gives (T5.1.6).
#[tokio::test]
async fn leaving_a_team_drops_access() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (zeynep, _) = world.person("Zeynep").await;
    let team = world
        .team(admin, &[(zeynep, teams::TeamRole::Member)])
        .await;
    let open = world
        .team_task(team, admin, items::TeamVisibilityMode::All)
        .await;
    let assigned = world
        .team_task_with(
            team,
            admin,
            items::TeamVisibilityMode::Assignees,
            &[zeynep],
            &[],
        )
        .await;
    let notebooks = world.notebooks();
    let notebook = notebooks
        .create_for_team(
            admin,
            team,
            notebooks::CreateTeamNotebookRequest {
                title: "Seçili".into(),
                description: None,
                visibility: Some(items::TeamVisibilityMode::Assignees),
                editors: Some(vec![zeynep]),
                viewers: None,
            },
        )
        .await
        .unwrap()
        .item
        .id;
    let page = notebooks
        .create_page(
            zeynep,
            notebook,
            notebooks::CreatePageRequest {
                title: "Benim".into(),
            },
        )
        .await
        .unwrap()
        .item
        .id;
    let items = world.items();
    for item in [open, assigned, notebook, page] {
        assert_eq!(items.get(zeynep, item).await.unwrap().id, item);
    }

    world.teams().leave(zeynep, team).await.unwrap();

    for item in [open, assigned, notebook, page] {
        assert!(
            matches!(items.get(zeynep, item).await, Err(AppError::NotFound)),
            "{item} is gone for her"
        );
        let kept = items.get(admin, item).await.unwrap();
        assert_eq!(kept.owner, items::Owner::Team { team_id: team });
    }
    assert!(
        items
            .list(zeynep, items::ListState::Active)
            .await
            .unwrap()
            .is_empty()
    );

    // Back in the team: the open item returns through her role, the chosen ones do not.
    let teams = world.teams();
    teams
        .invite(
            admin,
            team,
            teams::InviteToTeamRequest {
                user_id: zeynep,
                role: teams::TeamRole::Member,
            },
        )
        .await
        .unwrap();
    teams.accept(zeynep, team).await.unwrap();
    assert_eq!(items.get(zeynep, open).await.unwrap().id, open);
    for item in [assigned, notebook, page] {
        assert!(matches!(
            items.get(zeynep, item).await,
            Err(AppError::NotFound)
        ));
    }
}
