//! `V12` Team items are visible to the whole team or to assignees only. In "assignees only" mode the
//! creator, assignees and managers and above see the item.

use crate::support::pending;

/// A team task in "all team" mode is visible to every team member.
#[tokio::test]
#[ignore = "T5.1.3: team items and visibility mode"]
async fn all_team_mode_is_visible_to_members() {
    pending()
}

/// A team task in "assignees only" mode is visible to its creator, its assignees and managers and
/// admins, but not to other members.
#[tokio::test]
#[ignore = "T5.1.3: team items and visibility mode"]
async fn assignees_only_mode_limits_viewers() {
    pending()
}
