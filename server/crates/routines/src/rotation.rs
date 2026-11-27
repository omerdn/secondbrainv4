//! Rotation of a shared routine's responsible person, and swaps (PRD R6, ARCHITECTURE §8,
//! T3.3.2). The arithmetic is `recurrence::Rotation`; this module ties it to the routine's plan,
//! members and stored swaps.
//!
//! The people taking turns are the routine's current members (its assignees, or the owner when
//! nobody is assigned), in the stored order; someone not in the stored order joins at the end.
//! When someone leaves or joins, every later turn shifts, so the swaps made under the old order
//! stop counting (they are kept, not rewritten): nobody outside the routine is ever responsible,
//! and nobody is left holding half of a swap whose other half went away.

use access::Perms;
use chrono::{Days, NaiveDate};
use kernel::AppError;
use recurrence::{Rotation, RotationCadence, RotationError, Schedule, local_date};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::completions::{DAYS_MAX, internal, schedule_of};
use crate::model::{invalid, parse_timezone};
use crate::repo::{self, RoutineRow};
use crate::service::RoutinesService;

/// Days returned when the range is left out.
const DEFAULT_DAYS: u64 = 28;

/// When a shared routine's responsible person changes (R6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoutineRotation {
    /// The next person takes the next occurrence.
    EveryOccurrence,
    /// Everyone keeps all occurrences of their calendar week (Monday first).
    Weekly,
}

impl RoutineRotation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EveryOccurrence => "every_occurrence",
            Self::Weekly => "weekly",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "every_occurrence" => Some(Self::EveryOccurrence),
            "weekly" => Some(Self::Weekly),
            _ => None,
        }
    }

    const fn cadence(self) -> RotationCadence {
        match self {
            Self::EveryOccurrence => RotationCadence::EveryOccurrence,
            Self::Weekly => RotationCadence::Weekly,
        }
    }
}

/// The order the current members take turns in: the stored order without former members, then
/// anyone not in it yet, sorted so the result does not depend on how they were listed.
#[must_use]
pub fn turn_order(stored: &[Uuid], members: &[Uuid]) -> Vec<Uuid> {
    let mut order: Vec<Uuid> = Vec::with_capacity(members.len());
    for person in stored {
        if members.contains(person) && !order.contains(person) {
            order.push(*person);
        }
    }
    let mut newcomers: Vec<Uuid> = members
        .iter()
        .filter(|person| !order.contains(person))
        .copied()
        .collect();
    newcomers.sort_unstable();
    newcomers.dedup();
    order.extend(newcomers);
    order
}

/// `GET /routines/{id}/rotation` query; the next four weeks from today when absent.
#[derive(Debug, Clone, Copy, Default, Deserialize, TS)]
#[ts(export, rename = "RoutineRotationQuery")]
pub struct RotationQuery {
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub from: Option<NaiveDate>,
    #[serde(default)]
    #[ts(optional, type = "string")]
    pub to: Option<NaiveDate>,
}

/// `POST /routines/{id}/rotation/swaps` body: the people on the two occurrences trade places.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[ts(export, rename = "RoutineSwapRequest")]
pub struct SwapRequest {
    #[ts(type = "string")]
    pub first: NaiveDate,
    #[ts(type = "string")]
    pub second: NaiveDate,
}

/// One occurrence and who is responsible for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "RoutineAssignment")]
pub struct AssignmentView {
    #[ts(type = "string")]
    pub date: NaiveDate,
    pub user_id: Uuid,
}

/// A routine's rotation over a range of days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "RoutineRotationSchedule")]
pub struct RotationView {
    pub rotation: RoutineRotation,
    /// The people taking turns, first turn first.
    pub members: Vec<Uuid>,
    /// The occurrences in the range, in order, with their responsible person.
    pub assignments: Vec<AssignmentView>,
}

fn rotation_error(field: &str, err: RotationError) -> AppError {
    match err {
        RotationError::Schedule(err) => invalid(field, err.to_string()),
        other => invalid(field, other.to_string()),
    }
}

struct Loaded {
    kind: RoutineRotation,
    schedule: Schedule,
    rotation: Rotation<Uuid>,
    members: Vec<Uuid>,
    today: NaiveDate,
}

impl RoutinesService {
    /// The routine's current members in turn order.
    pub(crate) async fn turn_members(&self, id: Uuid) -> Result<Vec<Uuid>, AppError> {
        let stored = repo::rotation_order(&self.pool, id)
            .await
            .map_err(internal)?;
        Ok(turn_order(&stored, &self.people(id).await?))
    }

    /// The rotation as stored; [`AppError::Conflict`] when the routine has fixed assignees.
    async fn load_rotation(&self, row: &RoutineRow) -> Result<Loaded, AppError> {
        let kind = row
            .rotation
            .as_deref()
            .and_then(RoutineRotation::parse)
            .ok_or_else(|| AppError::Conflict("the routine has no rotation".into()))?;
        let timezone = parse_timezone(&row.timezone)?;
        let versions = repo::list_versions(&self.pool, row.item_id)
            .await
            .map_err(internal)?;
        let schedule = schedule_of(timezone, &versions)?;
        let members = self.turn_members(row.item_id).await?;
        let overrides = repo::rotation_overrides(&self.pool, row.item_id, &members)
            .await
            .map_err(internal)?;
        let rotation = Rotation::new(members.clone(), kind.cadence(), row.start_date)
            .and_then(|rotation| rotation.with_overrides(overrides))
            .map_err(internal)?;
        Ok(Loaded {
            kind,
            schedule,
            rotation,
            members,
            today: local_date(self.clock.now(), timezone),
        })
    }

    fn rotation_view(
        loaded: &Loaded,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<RotationView, AppError> {
        let assignments = loaded
            .rotation
            .assignments(&loaded.schedule, from, to)
            .map_err(|err| rotation_error("from", err))?;
        Ok(RotationView {
            rotation: loaded.kind,
            members: loaded.members.clone(),
            assignments: assignments
                .into_iter()
                .map(|a| AssignmentView {
                    date: a.date,
                    user_id: a.assignee,
                })
                .collect(),
        })
    }

    /// Who is responsible for each occurrence between `from` and `to` (VIEW, R6).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Conflict`] without a rotation,
    /// [`AppError::Validation`] for a bad range, or [`AppError::Internal`].
    pub async fn rotation(
        &self,
        actor: Uuid,
        id: Uuid,
        query: RotationQuery,
    ) -> Result<RotationView, AppError> {
        self.permissions
            .require(&self.pool, actor, id, Perms::VIEW)
            .await?;
        let row = repo::get_routine(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let loaded = self.load_rotation(&row).await?;
        let from = query.from.unwrap_or(loaded.today);
        let to = query
            .to
            .or_else(|| from.checked_add_days(Days::new(DEFAULT_DAYS - 1)))
            .unwrap_or(from);
        let span = (to - from).num_days();
        if !(0..DAYS_MAX).contains(&span) {
            return Err(invalid(
                "from",
                format!("the range must be 1 to {DAYS_MAX} days"),
            ));
        }
        Self::rotation_view(&loaded, from, to)
    }

    /// Trades the people on two upcoming occurrences (EDIT, R6). Someone may swap a turn of
    /// their own; a manager may swap anyone's. Everyone else keeps their turns, and swapping back
    /// leaves nothing stored.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`] for a turn that is not the actor's,
    /// [`AppError::Conflict`] without a rotation or in the trash, [`AppError::Validation`] for a
    /// past date, a day without an occurrence or a swap that changes nothing, or
    /// [`AppError::Internal`].
    pub async fn swap(
        &self,
        actor: Uuid,
        id: Uuid,
        request: SwapRequest,
    ) -> Result<RotationView, AppError> {
        self.require_live(actor, id).await?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        // One swap at a time, so two swaps never read the same overrides.
        let row = repo::lock_routine(&mut tx, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let mut loaded = self.load_rotation(&row).await?;
        for (field, date) in [("first", request.first), ("second", request.second)] {
            if date < loaded.today {
                return Err(invalid(field, "must not be in the past"));
            }
        }
        let on_first = loaded
            .rotation
            .assignee_on(&loaded.schedule, request.first)
            .map_err(|err| rotation_error("first", err))?;
        let on_second = loaded
            .rotation
            .assignee_on(&loaded.schedule, request.second)
            .map_err(|err| rotation_error("second", err))?;
        if actor != on_first
            && actor != on_second
            && !self
                .permissions
                .check(&self.pool, actor, id, Perms::MANAGE)
                .await
                .map_err(internal)?
        {
            return Err(AppError::Forbidden);
        }
        loaded
            .rotation
            .swap(&loaded.schedule, request.first, request.second)
            .map_err(|err| rotation_error("second", err))?;
        let dates = [request.first, request.second];
        let kept: Vec<(NaiveDate, Uuid)> = dates
            .iter()
            .filter_map(|date| {
                loaded
                    .rotation
                    .overrides()
                    .get(date)
                    .map(|person| (*date, *person))
            })
            .collect();
        repo::set_rotation_overrides(&mut tx, id, &dates, &kept, &loaded.members)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        let (from, to) = (dates[0].min(dates[1]), dates[0].max(dates[1]));
        Self::rotation_view(&loaded, from, to)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn stored_order_is_kept_and_newcomers_join_at_the_end() {
        let [a, b, c, d] = [1, 2, 3, 4].map(Uuid::from_u128);
        assert_eq!(turn_order(&[c, a], &[a, b, c]), vec![c, a, b]);
        assert_eq!(turn_order(&[], &[d, b]), vec![b, d]);
    }

    #[test]
    fn former_members_drop_out() {
        let [a, b, c] = [1, 2, 3].map(Uuid::from_u128);
        assert_eq!(turn_order(&[a, b, c], &[a, c]), vec![a, c]);
        assert!(turn_order(&[a], &[]).is_empty());
    }

    #[test]
    fn cadences_round_trip() {
        for kind in [RoutineRotation::EveryOccurrence, RoutineRotation::Weekly] {
            assert_eq!(RoutineRotation::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(RoutineRotation::parse("fixed"), None);
    }

    proptest! {
        /// The order is exactly the current members, each once, whatever was stored.
        #[test]
        fn order_is_a_permutation_of_the_members(
            stored in proptest::collection::vec(1u128..8, 0..10),
            members in proptest::collection::hash_set(1u128..8, 0..7),
        ) {
            let stored: Vec<Uuid> = stored.into_iter().map(Uuid::from_u128).collect();
            let members: Vec<Uuid> = members.into_iter().map(Uuid::from_u128).collect();
            let order = turn_order(&stored, &members);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            let mut expected = members.clone();
            expected.sort_unstable();
            prop_assert_eq!(sorted, expected);
        }
    }
}
