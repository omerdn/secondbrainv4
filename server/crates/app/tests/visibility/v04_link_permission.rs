//! `V4` Only the page owner may link a page to an item, and only to an item they can view. On team
//! pages the right can be given to authorised roles.

use crate::support::pending;

/// Ayşe can edit Ali's page through a link but does not own it: linking it to her own task fails.
#[tokio::test]
#[ignore = "T1.2.3: item links"]
async fn non_owner_cannot_link_a_page() {
    pending()
}

/// Ali owns page P but cannot see Ayşe's private task T: linking P to T fails and reveals nothing
/// about T.
#[tokio::test]
#[ignore = "T1.2.3: item links"]
async fn owner_cannot_link_to_an_item_they_cannot_see() {
    pending()
}

/// In team X the manager role is allowed to link team pages; a manager links one, a member cannot.
#[tokio::test]
#[ignore = "T5.1.5: team notebooks"]
async fn team_roles_can_be_allowed_to_link_team_pages() {
    pending()
}
