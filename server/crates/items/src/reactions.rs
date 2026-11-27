//! Emoji reactions to friends' completions (PRD AK4, T3.5.2). A person reacts to a "completed"
//! activity event of an item they can view (A2), done by a friend (V1's friends, never oneself).
//! One reaction per person and event; reacting again replaces it. A new reaction publishes
//! [`ReactionAdded`], which the notification center turns into the completer's notification.
//! Readers see counts per emoji and their own choice, never who else reacted.

use std::collections::{BTreeMap, HashMap};

use access::Perms;
use kernel::{AppError, DomainEvent, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::activity::ActivityKind;
use crate::service::ItemsService;

/// The emoji one may react with.
pub const REACTIONS: [&str; 6] = ["👏", "🔥", "💪", "🎉", "❤️", "👍"];

/// `PUT /activity/{id}/reaction` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "ReactRequest")]
pub struct ReactRequest {
    pub emoji: String,
}

/// How many people reacted with one emoji, and whether the reader is one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "ActivityReaction")]
pub struct ReactionCount {
    pub emoji: String,
    pub count: i64,
    pub mine: bool,
}

/// Someone reacted to a completion; published in the reaction's transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReactionAdded {
    pub activity_id: Uuid,
    pub item_id: Uuid,
    pub from_user: Uuid,
    /// Who completed it.
    pub to_user: Uuid,
    pub emoji: String,
}

impl DomainEvent for ReactionAdded {
    const EVENT_TYPE: &'static str = "activity.reaction_added";
}

fn invalid(field: &str, message: &str) -> AppError {
    ValidationErrors::from(FieldError::new(field, FieldErrorCode::Invalid, message)).into()
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Counts per emoji in [`REACTIONS`] order from `(user, emoji)` rows, with the reader's own marked.
#[must_use]
pub fn tally(rows: &[(Uuid, String)], reader: Uuid) -> Vec<ReactionCount> {
    let mut counts: BTreeMap<usize, (i64, bool)> = BTreeMap::new();
    for (user, emoji) in rows {
        let Some(at) = REACTIONS.iter().position(|e| e == emoji) else {
            continue;
        };
        let entry = counts.entry(at).or_default();
        entry.0 += 1;
        entry.1 |= *user == reader;
    }
    counts
        .into_iter()
        .map(|(at, (count, mine))| ReactionCount {
            emoji: REACTIONS[at].to_owned(),
            count,
            mine,
        })
        .collect()
}

impl ItemsService {
    /// The event `actor` may react to: a completion of an item they can view (A2), done by a
    /// friend. Its item and the person who completed it.
    async fn reactable(&self, actor: Uuid, activity: Uuid) -> Result<(Uuid, Uuid), AppError> {
        let row: Option<(Uuid, Option<Uuid>, String)> =
            sqlx::query_as("SELECT item_id, actor_id, type FROM activity_events WHERE id = $1")
                .bind(activity)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let (item, completer, kind) = row.ok_or(AppError::NotFound)?;
        // Not visible: the same answer as an unknown event (V9).
        self.require(actor, item, Perms::VIEW).await?;
        let Some(completer) = completer.filter(|_| kind == ActivityKind::Completed.as_str()) else {
            return Err(invalid("activity", "only a completion can get a reaction"));
        };
        if completer == actor {
            return Err(invalid(
                "activity",
                "you cannot react to your own completion",
            ));
        }
        if !self.sharing.can_share(actor, completer).await? {
            return Err(AppError::Forbidden);
        }
        Ok((item, completer))
    }

    /// Reacts to a friend's completion with one of [`REACTIONS`], replacing an earlier reaction
    /// (AK4). The event's counts after the change.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for an event the caller cannot see, [`AppError::Validation`] for
    /// another emoji, an event that is not a completion or one's own, [`AppError::Forbidden`]
    /// when the completer is not a friend, or [`AppError::Internal`].
    pub async fn react(
        &self,
        actor: Uuid,
        activity: Uuid,
        request: ReactRequest,
    ) -> Result<Vec<ReactionCount>, AppError> {
        if !REACTIONS.contains(&request.emoji.as_str()) {
            return Err(invalid("emoji", "not one of the offered reactions"));
        }
        let (item, completer) = self.reactable(actor, activity).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let changed = sqlx::query(
            "INSERT INTO activity_reactions (activity_id, user_id, emoji, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (activity_id, user_id) DO UPDATE
                 SET emoji = EXCLUDED.emoji, created_at = EXCLUDED.created_at
                 WHERE activity_reactions.emoji <> EXCLUDED.emoji",
        )
        .bind(activity)
        .bind(actor)
        .bind(&request.emoji)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(internal)?
        .rows_affected()
            == 1;
        if changed {
            db::outbox::publish(
                &mut tx,
                &ReactionAdded {
                    activity_id: activity,
                    item_id: item,
                    from_user: actor,
                    to_user: completer,
                    emoji: request.emoji,
                },
            )
            .await
            .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(self
            .reactions(actor, &[activity])
            .await?
            .remove(&activity)
            .unwrap_or_default())
    }

    /// Takes the caller's reaction back. Nothing to take back changes nothing.
    ///
    /// # Errors
    /// As [`Self::react`] for the event.
    pub async fn unreact(
        &self,
        actor: Uuid,
        activity: Uuid,
    ) -> Result<Vec<ReactionCount>, AppError> {
        self.reactable(actor, activity).await?;
        sqlx::query("DELETE FROM activity_reactions WHERE activity_id = $1 AND user_id = $2")
            .bind(activity)
            .bind(actor)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(self
            .reactions(actor, &[activity])
            .await?
            .remove(&activity)
            .unwrap_or_default())
    }

    /// The reactions of events the caller already sees, by event.
    pub(crate) async fn reactions(
        &self,
        reader: Uuid,
        activities: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ReactionCount>>, AppError> {
        let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
            "SELECT activity_id, user_id, emoji FROM activity_reactions
             WHERE activity_id = ANY($1)",
        )
        .bind(activities)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut by_event: HashMap<Uuid, Vec<(Uuid, String)>> = HashMap::new();
        for (activity, user, emoji) in rows {
            by_event.entry(activity).or_default().push((user, emoji));
        }
        Ok(by_event
            .into_iter()
            .map(|(activity, rows)| (activity, tally(&rows, reader)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ak4_counts_follow_the_offered_order_and_mark_the_readers_own() {
        let (ali, ayse, can) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let rows = vec![
            (ayse, "🎉".to_owned()),
            (ali, "👏".to_owned()),
            (can, "🎉".to_owned()),
            (can, "not offered".to_owned()),
        ];
        assert_eq!(
            tally(&rows, ayse),
            [
                ReactionCount {
                    emoji: "👏".into(),
                    count: 1,
                    mine: false
                },
                ReactionCount {
                    emoji: "🎉".into(),
                    count: 2,
                    mine: true
                },
            ]
        );
        assert!(tally(&[], ali).is_empty());
    }
}
