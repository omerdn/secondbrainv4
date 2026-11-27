//! `V11` "Why am I seeing this?": a user can see the path through which they access an object.

use access::{Reason, Relation as R};

use crate::support::{World, id, t};

/// Ayşe sees page P both as a notebook member and through a link to task T. The explanation lists
/// both reasons; for Ali it says "owner".
#[tokio::test]
async fn explanation_lists_every_access_path() {
    let world = World::new().await;
    let (ali, ayse, notebook, page, task) = (id(), id(), id(), id(), id());
    world
        .grant(&[
            t(notebook, R::Owner, ali),
            t(notebook, R::Viewer, ayse),
            t(page, R::Owner, ali),
            t(page, R::InNotebook, notebook),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(page, R::LinkedTo, task),
        ])
        .await;
    let permissions = world.permissions();
    let ayse_view = permissions
        .explain(world.pool(), ayse, page)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ayse_view.reasons,
        [
            Reason::Notebook {
                notebook_id: notebook
            },
            Reason::Linked { item_id: task },
        ]
    );
    let ali_view = permissions
        .explain(world.pool(), ali, page)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ali_view.reasons[0], Reason::Owner);
}

/// Deniz cannot see page P: asking why gives no explanation at all, so the question cannot be used
/// to learn that P exists or who can see it (V9).
#[tokio::test]
async fn no_explanation_for_what_one_cannot_see() {
    let world = World::new().await;
    let (ali, ayse, deniz, page, task) = (id(), id(), id(), id(), id());
    world
        .grant(&[
            t(page, R::Owner, ali),
            t(task, R::Owner, ali),
            t(task, R::Assignee, ayse),
            t(page, R::LinkedTo, task),
        ])
        .await;
    let permissions = world.permissions();
    assert!(
        permissions
            .explain(world.pool(), deniz, page)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        permissions
            .explain(world.pool(), ayse, page)
            .await
            .unwrap()
            .unwrap()
            .reasons,
        [Reason::Linked { item_id: task }]
    );
}
