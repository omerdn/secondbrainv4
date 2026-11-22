//! `V9` A user who can see a page but not one of its linked items learns nothing about that link:
//! no title, no count, no placeholder.

use crate::support::pending;

/// Page P is linked to tasks T1 (visible to Ayşe) and T2 (not visible to her). Listing P's links as
/// Ayşe returns only T1, and no count or marker hints at T2.
#[tokio::test]
#[ignore = "T1.2.3: item links"]
async fn invisible_link_targets_are_omitted_entirely() {
    pending()
}
