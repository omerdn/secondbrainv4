//! `V3` Access inherited through a link is view + edit only: never share, link, manage or delete.

use crate::support::pending;

/// Ayşe sees task T; Ali's page P is linked to T. Ayşe can view and edit P, but sharing it,
/// linking it elsewhere, managing its members and deleting it are all forbidden.
#[tokio::test]
#[ignore = "T3.2.2: inherited visibility end to end"]
async fn link_inheritance_grants_view_and_edit_only() {
    pending()
}
