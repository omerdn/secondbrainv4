//! `V1` Sharing prerequisite: an item can only be shared with a friend or a teammate.

use crate::support::pending;

/// Ali owns a task; Ayşe is neither his friend nor in any of his teams. Inviting her is rejected
/// and no relation is created.
#[tokio::test]
#[ignore = "T3.2.1: item member invitations"]
async fn sharing_with_a_stranger_is_rejected() {
    pending()
}

/// Ali and Ayşe are friends. Inviting Ayşe succeeds; after she accepts she can view the task.
#[tokio::test]
#[ignore = "T3.2.1: item member invitations"]
async fn sharing_with_a_friend_is_allowed() {
    pending()
}

/// Ali and Mehmet are in the same team but not friends. Ali can share a personal task with him.
#[tokio::test]
#[ignore = "T5.1.1: teams and membership"]
async fn sharing_with_a_teammate_is_allowed() {
    pending()
}
