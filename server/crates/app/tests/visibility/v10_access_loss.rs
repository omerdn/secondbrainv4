//! `V10` When a user loses access, their open sessions on that object are cut immediately.

use crate::support::pending;

/// Ayşe has page P open over the realtime channel. Ali removes her access; her subscription to P
/// is terminated and further updates are not delivered.
#[tokio::test]
#[ignore = "T1.1.6: AccessRevoked event and realtime disconnect"]
async fn revoked_user_is_disconnected() {
    pending()
}
