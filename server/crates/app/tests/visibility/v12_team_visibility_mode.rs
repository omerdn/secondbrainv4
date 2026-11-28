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

/// T5.1.8: tasks, routines, goals and trackers are created for a team through their own
/// endpoints; "assignees only" works the same for each kind.
#[tokio::test]
async fn every_kind_can_belong_to_a_team() {
    use std::sync::Arc;
    let world = World::new().await;
    let (team, [admin, _, assignee, other, _, _]) = team_of_six(&world).await;
    let clock: kernel::SharedClock = Arc::new(kernel::SystemClock);
    let placement = || {
        Some(items::TeamPlacementRequest {
            team_id: team,
            visibility: Some(TeamVisibilityMode::Assignees),
            assignees: Some(vec![assignee]),
            watchers: None,
        })
    };
    let (pool, permissions, items) = (world.pool().clone(), world.permissions(), world.items());
    let task = tasks::TasksService::new(
        pool.clone(),
        clock.clone(),
        permissions.clone(),
        items.clone(),
    )
    .create(
        admin,
        tasks::CreateTaskRequest {
            team: placement(),
            title: "Rapor".into(),
            description: None,
            start_date: None,
            due_date: None,
            subtasks: None,
        },
    )
    .await
    .unwrap()
    .item;
    let routine = routines::RoutinesService::new(
        pool.clone(),
        clock.clone(),
        permissions.clone(),
        items.clone(),
    )
    .create(
        admin,
        routines::CreateRoutineRequest {
            team: placement(),
            title: "Toplantı".into(),
            description: None,
            timezone: "UTC".into(),
            rrule: "FREQ=WEEKLY".into(),
            daily_target: None,
            start_date: None,
            end_date: None,
            reminder_times: None,
            completion_mode: None,
            rotation: None,
        },
    )
    .await
    .unwrap()
    .item;
    let today = chrono::Utc::now().date_naive();
    let goal = goals::GoalsService::new(
        pool.clone(),
        clock.clone(),
        permissions.clone(),
        items.clone(),
    )
    .create(
        admin,
        goals::CreateGoalRequest {
            team: placement(),
            title: "Çeyrek".into(),
            description: None,
            start_date: Some(today),
            end_date: Some(today + chrono::Duration::days(30)),
        },
    )
    .await
    .unwrap()
    .item;
    let tracker = tracking::TrackerEntries::new(
        pool.clone(),
        clock.clone(),
        permissions.clone(),
        items.clone(),
        world.identity_service(),
    )
    .create_tracker(
        admin,
        tracking::CreateTrackerRequest {
            team: placement(),
            title: "Satış".into(),
            description: None,
            unit: "adet".into(),
            target_value: rust_decimal::Decimal::from(10),
            period: tracking::Period::Week,
            aggregation: tracking::Aggregation::Sum,
            direction: tracking::Direction::Increase,
        },
    )
    .await
    .unwrap()
    .item;
    for item in [task, routine, goal, tracker] {
        assert_eq!(
            item.owner,
            items::Owner::Team { team_id: team },
            "{}",
            item.kind
        );
        assert!(sees(&world, assignee, item.id).await, "{}", item.kind);
        assert!(!sees(&world, other, item.id).await, "{}", item.kind);
    }
}

/// TK2 (T5.1.8): teammates join a team item directly, without an invitation. Choosing someone
/// else needs SHARE (the role matrix); choosing oneself needs EDIT; only teammates can be chosen,
/// and personal items keep inviting.
#[tokio::test]
async fn teammates_are_added_directly() {
    let world = World::new().await;
    let (team, [admin, manager, member, other, _, guest]) = team_of_six(&world).await;
    let (outsider, _) = world.person("Dışarıdan").await;
    let item = world
        .team_task(team, admin, TeamVisibilityMode::Assignees)
        .await;
    let items = world.items();
    use items::InvitationRole::{Assignee, Editor, Watcher};

    // `other` does not see it: for him it does not exist.
    assert!(matches!(
        items.add_team_member(other, item, other, Assignee).await,
        Err(AppError::NotFound)
    ));
    // The manager (SHARE) assigns a member; she sees it at once.
    items
        .add_team_member(manager, item, member, Assignee)
        .await
        .unwrap();
    assert!(sees(&world, member, item).await);
    // Again is a no-op; a member cannot choose others, but may watch herself.
    items
        .add_team_member(manager, item, member, Assignee)
        .await
        .unwrap();
    assert!(matches!(
        items.add_team_member(member, item, other, Watcher).await,
        Err(AppError::Forbidden)
    ));
    items
        .add_team_member(member, item, member, Watcher)
        .await
        .unwrap();
    for (who, role) in [(outsider, Assignee), (guest, Editor)] {
        assert!(matches!(
            items.add_team_member(admin, item, who, role).await,
            Err(AppError::Validation(_))
        ));
    }
    let personal = world.item(admin, "task", "Kişisel").await;
    assert!(matches!(
        items
            .add_team_member(admin, personal.id, member, Assignee)
            .await,
        Err(AppError::Validation(_))
    ));
    let members = items
        .members(admin, item, |ids| async move {
            Ok(ids.into_iter().map(|id| (id, String::new())).collect())
        })
        .await
        .unwrap();
    let theirs = members
        .members
        .iter()
        .find(|m| m.user_id == member)
        .unwrap();
    assert_eq!(
        theirs.roles,
        vec![items::MemberRole::Assignee, items::MemberRole::Watcher]
    );
}
