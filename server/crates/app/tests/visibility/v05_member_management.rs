//! `V5` Only the owner may add members to a shared item; in teams the role matrix decides.

use items::{CreateInvitationRequest, InvitationRole};
use kernel::AppError;

use crate::support::World;

/// Ayşe is an assignee of Ali's task. Her attempt to invite Can is forbidden.
#[tokio::test]
async fn only_the_owner_adds_members() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayse").await;
    let (can, can_email) = world.person("Can").await;
    world.befriend(ali, ayse, &ayse_email).await;
    world.befriend(ayse, can, &can_email).await;
    world.befriend(ali, can, &can_email).await;
    let items = world.sharing_items();
    let task = world.item(ali, "task", "T").await;
    let invitation = items
        .invite(
            ali,
            task.id,
            CreateInvitationRequest {
                to_user: ayse,
                role: InvitationRole::Assignee,
            },
        )
        .await
        .unwrap();
    items.accept_invitation(ayse, invitation.id).await.unwrap();

    // Ayşe is an assignee and Can's friend, yet she cannot add him.
    let attempt = items
        .invite(
            ayse,
            task.id,
            CreateInvitationRequest {
                to_user: can,
                role: InvitationRole::Watcher,
            },
        )
        .await;
    assert!(matches!(attempt, Err(AppError::Forbidden)));
    assert!(items.pending_invitations(can).await.unwrap().is_empty());
    // Nor remove Ali's other members, nor see pending invitations; she may only leave.
    let members = items
        .members(ayse, task.id, |_| async {
            Ok(std::collections::HashMap::new())
        })
        .await
        .unwrap();
    assert!(!members.can_manage && members.pending.is_empty());
    assert!(matches!(
        items.remove_member(ayse, task.id, ali).await,
        Err(AppError::Forbidden | AppError::Validation(_))
    ));
    items.remove_member(ayse, task.id, ayse).await.unwrap();
    assert!(matches!(
        items.get(ayse, task.id).await,
        Err(AppError::NotFound)
    ));
}

/// Team X's admin grants member management to managers. A manager can add an assignee to a team
/// task; a plain member cannot. When the admin takes it back, the manager cannot either (V8).
#[tokio::test]
async fn team_role_matrix_controls_member_management() {
    let world = World::new().await;
    let (admin, _) = world.person("Admin").await;
    let (mehmet, _) = world.person("Mehmet").await;
    let (zeynep, _) = world.person("Zeynep").await;
    let (deniz, _) = world.person("Deniz").await;
    let team = world
        .team(
            admin,
            &[
                (mehmet, teams::TeamRole::Manager),
                (zeynep, teams::TeamRole::Member),
                (deniz, teams::TeamRole::Member),
            ],
        )
        .await;
    let task = world
        .team_task(team, admin, items::TeamVisibilityMode::All)
        .await;
    let teams = world.teams();
    let mut matrix = teams.matrix(admin, team).await.unwrap();
    for row in &mut matrix.roles {
        match row.role {
            teams::TeamRole::Manager => {
                row.item = vec![
                    teams::ItemPermission::View,
                    teams::ItemPermission::Edit,
                    teams::ItemPermission::Share,
                ];
            }
            teams::TeamRole::Member => {
                row.item = vec![teams::ItemPermission::View, teams::ItemPermission::Edit];
            }
            _ => {}
        }
    }
    teams.set_matrix(admin, team, matrix.clone()).await.unwrap();
    let items = world.sharing_items();
    let assignee = |to_user| CreateInvitationRequest {
        to_user,
        role: InvitationRole::Assignee,
    };
    assert!(matches!(
        items.invite(zeynep, task, assignee(deniz)).await,
        Err(AppError::Forbidden)
    ));
    items.invite(mehmet, task, assignee(deniz)).await.unwrap();

    for row in &mut matrix.roles {
        if row.role == teams::TeamRole::Manager {
            row.item = vec![teams::ItemPermission::View, teams::ItemPermission::Edit];
        }
    }
    teams.set_matrix(admin, team, matrix).await.unwrap();
    assert!(matches!(
        items.invite(mehmet, task, assignee(zeynep)).await,
        Err(AppError::Forbidden)
    ));
}

/// Ali sees his task's members with their roles and the invitation Can has not answered; when Can
/// declines nothing changes (G5), and Can never sees the task.
#[tokio::test]
async fn the_owner_sees_members_and_pending_invitations() {
    let world = World::new().await;
    let (ali, _) = world.person("Ali").await;
    let (ayse, ayse_email) = world.person("Ayse").await;
    let (can, can_email) = world.person("Can").await;
    world.befriend(ali, ayse, &ayse_email).await;
    world.befriend(ali, can, &can_email).await;
    let items = world.sharing_items();
    let task = world.item(ali, "task", "T").await;
    let to_ayse = items
        .invite(
            ali,
            task.id,
            CreateInvitationRequest {
                to_user: ayse,
                role: InvitationRole::Assignee,
            },
        )
        .await
        .unwrap();
    items.accept_invitation(ayse, to_ayse.id).await.unwrap();
    let to_can = items
        .invite(
            ali,
            task.id,
            CreateInvitationRequest {
                to_user: can,
                role: InvitationRole::Watcher,
            },
        )
        .await
        .unwrap();
    let names = |ids: Vec<uuid::Uuid>| async move {
        Ok(ids.into_iter().map(|id| (id, id.to_string())).collect())
    };

    let members = items.members(ali, task.id, names).await.unwrap();
    assert!(members.can_manage);
    let roles: Vec<(uuid::Uuid, Vec<items::MemberRole>)> = members
        .members
        .iter()
        .map(|m| (m.user_id, m.roles.clone()))
        .collect();
    assert_eq!(
        roles,
        [
            (ali, vec![items::MemberRole::Owner]),
            (ayse, vec![items::MemberRole::Assignee])
        ]
    );
    assert_eq!(members.pending.len(), 1);
    assert_eq!(members.pending[0].user_id, can);
    assert_eq!(members.pending[0].display_name, can.to_string());

    items.decline_invitation(can, to_can.id).await.unwrap();
    let members = items.members(ali, task.id, names).await.unwrap();
    assert!(members.pending.is_empty());
    assert_eq!(members.members.len(), 2);
    assert!(matches!(
        items.get(can, task.id).await,
        Err(AppError::NotFound)
    ));
    // Can sees no members of a task he cannot view.
    assert!(matches!(
        items.members(can, task.id, names).await,
        Err(AppError::NotFound)
    ));
}
