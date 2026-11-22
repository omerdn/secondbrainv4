//! `V6` Only the owner may delete a page or an item.

use crate::support::pending;

/// Ayşe is an assignee of Ali's task and can edit it, but deleting it is forbidden; Ali can.
#[tokio::test]
#[ignore = "T1.2.1: items CRUD with trash"]
async fn only_the_owner_deletes_an_item() {
    pending()
}

/// Ayşe edits Ali's page through a link; deleting the page is forbidden.
#[tokio::test]
#[ignore = "T3.2.2: inherited visibility end to end"]
async fn link_editors_cannot_delete_a_page() {
    pending()
}
