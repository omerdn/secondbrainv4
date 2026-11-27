//! `V12` Team items are visible to the whole team or to assignees only. In "assignees only" mode the
//! creator, assignees and managers and above see the item.

use items::TeamVisibilityMode;
use kernel::AppError;
use teams::TeamRole;
use uuid::Uuid;

use crate::support::World;

/// A team of an admin, a manager, three members and a guest; their ids in that order.
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

async fn sees(world: &World, user: Uuid, item: Uuid) -> bool {
    match world.items().get(user, item).await {
        Ok(found) => found.id == item,
        Err(AppError::NotFound) => false,
        Err(err) => panic!("{err}"),
    }
}

/// A team task in "all team" mode is visible to every team member.
#[tokio::test]
async fn all_team_mode_is_visible_to_members() {
    let world = World::new().await;
    let (team, people) = team_of_six(&world).await;
    let [_, _, creator, ..] = people;
    let (outsider, _) = world.person("Dışarıdan").await;
    let item = world
        .team_task(team, creator, TeamVisibilityMode::All)
        .await;

    for person in people {
        assert!(sees(&world, person, item).await, "{person} sees the item");
    }
    assert!(!sees(&world, outsider, item).await);
    let read = world.items().get(creator, item).await.unwrap();
    assert_eq!(read.owner, items::Owner::Team { team_id: team });
    assert_eq!(read.team_visibility, Some(TeamVisibilityMode::All));
    assert_eq!(read.created_by, creator);
}

/// A team task in "assignees only" mode is visible to its creator, its assignees and managers and
/// admins, but not to other members.
#[tokio::test]
async fn assignees_only_mode_limits_viewers() {
    let world = World::new().await;
    let (team, [admin, manager, creator, assignee, other, guest]) = team_of_six(&world).await;
    let item = world
        .team_task_with(
            team,
            manager,
            TeamVisibilityMode::Assignees,
            &[assignee],
            &[guest],
        )
        .await;
    // The manager created it without being an assignee (TK4, N3).
    for person in [admin, manager, assignee, guest] {
        assert!(sees(&world, person, item).await, "{person} sees the item");
    }
    for person in [creator, other] {
        assert!(
            !sees(&world, person, item).await,
            "{person} must not see it"
        );
    }

    // A member's own item: the creator sees it even though they are not an assignee.
    let own = world
        .team_task(team, creator, TeamVisibilityMode::Assignees)
        .await;
    for person in [admin, manager, creator] {
        assert!(sees(&world, person, own).await, "{person} sees the item");
    }
    for person in [assignee, other, guest] {
        assert!(!sees(&world, person, own).await, "{person} must not see it");
    }
    // Nothing leaks through lists or search either.
    let listed = world
        .items()
        .list(other, items::ListState::Active)
        .await
        .unwrap();
    assert!(listed.iter().all(|i| i.id != item && i.id != own));
}

/// Choosing someone else needs the role's "assign" right; a member may still assign themselves.
/// Outsiders cannot tell a team exists, guests may not create items, and assignees must be
/// members (TK3, TK4).
#[tokio::test]
async fn creating_team_items_follows_the_matrix() {
    let world = World::new().await;
    let (team, [admin, _, member, other, _, guest]) = team_of_six(&world).await;
    let (outsider, _) = world.person("Dışarıdan").await;
    let items = world.items();
    let new = |assignees: Vec<Uuid>| {
        items::CreateTeamItemRequest {
            kind: "task".into(),
            title: "İş".into(),
            description: None,
            visibility: None,
            assignees: Some(assignees),
            watchers: None,
        }
        .validate()
        .unwrap()
    };

    let unknown = items
        .create_team_item(outsider, Uuid::now_v7(), new(vec![]))
        .await;
    let foreign = items.create_team_item(outsider, team, new(vec![])).await;
    assert!(matches!(unknown, Err(AppError::NotFound)));
    assert!(matches!(foreign, Err(AppError::NotFound)));
    assert!(matches!(
        items.create_team_item(guest, team, new(vec![])).await,
        Err(AppError::Forbidden)
    ));
    assert!(matches!(
        items.create_team_item(member, team, new(vec![other])).await,
        Err(AppError::Forbidden)
    ));
    let mine = items
        .create_team_item(member, team, new(vec![member]))
        .await
        .unwrap();
    assert_eq!(mine.team_visibility, Some(TeamVisibilityMode::All));
    assert!(matches!(
        items
            .create_team_item(admin, team, new(vec![outsider]))
            .await,
        Err(AppError::Validation(_))
    ));
}
