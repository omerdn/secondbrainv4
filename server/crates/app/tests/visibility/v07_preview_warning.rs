//! `V7` Before any action that widens visibility, the users who would gain access are listed.

use crate::support::pending;

/// Task T is shared with Ayşe and Mehmet. Previewing "link page P to T" lists exactly Ayşe and
/// Mehmet, and changes nothing.
#[tokio::test]
#[ignore = "T1.1.5: PermissionService preview"]
async fn linking_preview_lists_new_viewers() {
    pending()
}

/// Task T has two linked pages. Previewing "add Can to T" reports Can gaining T and both pages.
#[tokio::test]
#[ignore = "T3.2.2: inherited visibility end to end"]
async fn adding_a_member_previews_linked_pages() {
    pending()
}
