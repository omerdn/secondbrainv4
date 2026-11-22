//! `V2` Page viewers = notebook members ∪ viewers of *every* item the page is linked to.

use crate::support::pending;

/// Page P is in Ali's notebook and linked to tasks T1 (shared with Ayşe) and T2 (shared with Can).
/// Ali, Ayşe and Can can all view P; Deniz, who sees neither task, cannot.
#[tokio::test]
#[ignore = "T3.2.2: inherited visibility end to end"]
async fn viewers_of_each_linked_item_see_the_page() {
    pending()
}

/// Ali adds Ayşe to his notebook as a viewer. She sees every page in it, linked or not.
#[tokio::test]
#[ignore = "T3.4.1: notebook membership API"]
async fn notebook_members_see_its_pages() {
    pending()
}
