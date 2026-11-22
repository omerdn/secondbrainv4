//! `V11` "Why am I seeing this?": a user can see the path through which they access an object.

use crate::support::pending;

/// Ayşe sees page P both as a notebook member and through a link to task T. The explanation lists
/// both reasons; for Ali it says "owner".
#[tokio::test]
#[ignore = "T1.1.5: PermissionService explain"]
async fn explanation_lists_every_access_path() {
    pending()
}
