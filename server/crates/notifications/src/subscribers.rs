//! Notifications made from other modules' outbox events (T2.8.1). Each handler reads what it
//! shows through the owning module's service (ADR-0007) and passes the event id, so a
//! redelivery makes nothing new.

use db::outbox::{Dispatcher, HandlerError};
use identity::IdentityService;
use items::{InvitationCreated, ItemsService};

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
                Ok(())
            }
        },
    );
}
