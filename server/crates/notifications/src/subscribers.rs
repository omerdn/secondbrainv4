//! Notifications made from other modules' outbox events (T2.8.1). Each handler reads what it
//! shows through the owning module's service (ADR-0007) and passes the event id, so a
//! redelivery makes nothing new.

use db::outbox::{Dispatcher, HandlerError};
use identity::IdentityService;
use items::{InvitationCreated, ItemsService, ReactionAdded};
use routines::reminders::RoutineReminderDue;
use social::{FriendRequestSent, Nudged};

use crate::model::NotificationPayload;
use crate::service::Notifications;

/// Subscribes the notification center to the events it turns into notifications (call from
/// `app::run`).
pub fn register(
    dispatcher: &mut Dispatcher,
    notifications: Notifications,
    items: ItemsService,
    identity: IdentityService,
) {
    let reminders = notifications.clone();
    let (nudges, nudge_names) = (notifications.clone(), identity.clone());
    let (requests, request_names) = (notifications.clone(), identity.clone());
    let (reactions, reaction_items, reaction_names) =
        (notifications.clone(), items.clone(), identity.clone());
    dispatcher.subscribe(
        "notifications.invitation",
        move |event: InvitationCreated, stored| {
            let (notifications, items, identity) =
                (notifications.clone(), items.clone(), identity.clone());
            async move {
                let boxed = |err: kernel::AppError| Box::new(err) as HandlerError;
                // Still pending: one answered or expired before delivery needs no notification.
                let pending = items
                    .pending_invitations(event.to_user)
                    .await
                    .map_err(boxed)?;
                let Some(invitation) = pending.into_iter().find(|i| i.id == event.invitation_id)
                else {
                    return Ok(());
                };
                let from_name = identity
                    .display_names(&[event.from_user])
                    .await
                    .map_err(boxed)?
                    .remove(&event.from_user)
                    .unwrap_or_default();
                let template = crate::email::Template::Invitation {
                    from_name: from_name.clone(),
                    item_kind: invitation.item_kind.clone(),
                    item_title: invitation.item_title.clone(),
                    role: invitation.role.as_str().to_owned(),
                };
                let payload = NotificationPayload::Invitation {
                    invitation_id: invitation.id,
                    item_id: invitation.item_id,
                    item_kind: invitation.item_kind,
                    item_title: invitation.item_title,
                    from_user: invitation.from_user,
                    from_name,
                    role: invitation.role,
                };
                notifications
                    .deliver(event.to_user, &payload, stored.id)
                    .await
                    .map_err(boxed)?;
                // BL1: an invitation waits for an answer, so it may also go by e-mail (BL3).
                notifications
                    .queue_email(
                        event.to_user,
                        crate::model::NotificationKind::Invitation,
                        template,
                        stored.id,
                    )
                    .await
                    .map_err(boxed)?;
                Ok(())
            }
        },
    );
    {
        dispatcher.subscribe(
            "notifications.routine-reminder",
            move |event: RoutineReminderDue, stored| {
                let notifications = reminders.clone();
                async move {
                    let payload = NotificationPayload::RoutineReminder {
                        routine_id: event.routine_id,
                        title: event.title,
                        local_date: event.local_date,
                        time: event.time,
                    };
                    notifications
                        .deliver(event.user_id, &payload, stored.id)
                        .await
                        .map_err(|err| Box::new(err) as HandlerError)?;
                    Ok(())
                }
            },
        );
    }
    dispatcher.subscribe(
        "notifications.reaction",
        move |event: ReactionAdded, stored| {
            let (notifications, items, identity) = (
                reactions.clone(),
                reaction_items.clone(),
                reaction_names.clone(),
            );
            async move {
                let boxed = |err: kernel::AppError| Box::new(err) as HandlerError;
                // V9: someone who can no longer see the item learns nothing about it.
                let item = match items.get(event.to_user, event.item_id).await {
                    Ok(item) => item,
                    Err(kernel::AppError::NotFound) => return Ok(()),
                    Err(err) => return Err(boxed(err)),
                };
                let from_name = identity
                    .display_names(&[event.from_user])
                    .await
                    .map_err(boxed)?
                    .remove(&event.from_user)
                    .unwrap_or_default();
                let payload = NotificationPayload::Reaction {
                    activity_id: event.activity_id,
                    item_id: item.id,
                    item_kind: item.kind,
                    item_title: item.title,
                    from_user: event.from_user,
                    from_name,
                    emoji: event.emoji,
                };
                notifications
                    .deliver(event.to_user, &payload, stored.id)
                    .await
                    .map_err(boxed)?;
                Ok(())
            }
        },
    );
    // A friend request has its own list in the app; it only adds an e-mail (BL1, T3.7.1).
    dispatcher.subscribe(
        "notifications.friend-request-email",
        move |event: FriendRequestSent, stored| {
            let (notifications, identity) = (requests.clone(), request_names.clone());
            async move {
                let boxed = |err: kernel::AppError| Box::new(err) as HandlerError;
                let from_name = identity
                    .display_names(&[event.from_user])
                    .await
                    .map_err(boxed)?
                    .remove(&event.from_user)
                    .unwrap_or_default();
                notifications
                    .queue_email(
                        event.to_user,
                        crate::model::NotificationKind::Invitation,
                        crate::email::Template::FriendRequest { from_name },
                        stored.id,
                    )
                    .await
                    .map_err(boxed)?;
                Ok(())
            }
        },
    );
    dispatcher.subscribe("notifications.nudge", move |event: Nudged, stored| {
        let (notifications, identity) = (nudges.clone(), nudge_names.clone());
        async move {
            let boxed = |err: kernel::AppError| Box::new(err) as HandlerError;
            let from_name = identity
                .display_names(&[event.from_user])
                .await
                .map_err(boxed)?
                .remove(&event.from_user)
                .unwrap_or_default();
            let payload = NotificationPayload::Nudge {
                from_user: event.from_user,
                from_name,
            };
            notifications
                .deliver(event.to_user, &payload, stored.id)
                .await
                .map_err(boxed)?;
            Ok(())
        }
    });
}
