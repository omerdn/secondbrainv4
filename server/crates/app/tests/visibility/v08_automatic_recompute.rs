//! `V8` Access is recomputed on every relevant change; access that came through a removed link
//! drops automatically.

use crate::support::pending;

/// Ayşe sees page P only through its link to task T. After Ali unlinks P from T she can no longer
/// see P.
#[tokio::test]
#[ignore = "T1.2.3: item links"]
async fn unlinking_drops_inherited_access() {
    pending()
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
