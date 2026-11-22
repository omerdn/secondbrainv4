//! `V5` Only the owner may add members to a shared item; in teams the role matrix decides.

use crate::support::pending;

/// Ayşe is an assignee of Ali's task. Her attempt to invite Can is forbidden.
#[tokio::test]
#[ignore = "T3.2.1: item member invitations"]
async fn only_the_owner_adds_members() {
    pending()
}

/// Team X's admin grants member management to managers. A manager can add an assignee to a team
/// task; a plain member cannot.
#[tokio::test]
#[ignore = "T5.1.2: team roles and permission matrix"]
async fn team_role_matrix_controls_member_management() {
    pending()
}
