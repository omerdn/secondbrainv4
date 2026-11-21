//! Typed UUIDv7 identifiers (ARCHITECTURE §5: "Tüm id'ler UUIDv7").
//!
//! `Id<T>` carries a phantom marker so ids of different entities cannot be mixed up:
//!
//! ```
//! use kernel::Id;
//!
//! pub enum User {}
//! pub type UserId = Id<User>;
//!
//! let id = UserId::new();
//! let parsed: UserId = id.to_string().parse().unwrap();
//! assert_eq!(id, parsed);
//! ```
//!
//! UUIDv7 embeds a millisecond timestamp, so ids sort by creation time.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// A UUIDv7 identifier for entities of type `T`.
///
/// `T` is only a marker (usually an empty enum); it does not need to implement anything.
pub struct Id<T> {
    uuid: Uuid,
    // `fn() -> T` keeps `Id<T>` `Send + Sync + Copy` regardless of `T`.
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    /// Generates a new id for the current system time.
    #[allow(clippy::new_without_default)] // a random id is not a sensible `Default`
    pub fn new() -> Self {
        Self::from_uuid(Uuid::now_v7())
    }

    /// Generates a new id whose timestamp is `at` (e.g. from an injected [`Clock`]).
    ///
    /// [`Clock`]: crate::Clock
    pub fn new_at(at: DateTime<Utc>) -> Self {
        let millis = u64::try_from(at.timestamp_millis()).unwrap_or(0);
        let ts = uuid::Timestamp::from_unix(
            uuid::NoContext,
            millis / 1000,
            u32::try_from((millis % 1000) * 1_000_000).unwrap_or(0),
        );
        Self::from_uuid(Uuid::new_v7(ts))
    }

    /// Wraps an existing UUID (e.g. read from the database). No version check is performed.
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self {
            uuid,
            _marker: PhantomData,
        }
    }

    /// The underlying UUID.
    pub const fn as_uuid(&self) -> Uuid {
        self.uuid
    }
}

// Manual impls: derives would require `T: Trait` bounds on the marker type.

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.uuid.cmp(&other.uuid)
    }
}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uuid.hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.uuid)
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.uuid.hyphenated(), f)
    }
}

impl<T> FromStr for Id<T> {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self::from_uuid)
    }
}

impl<T> From<Id<T>> for Uuid {
    fn from(id: Id<T>) -> Self {
        id.uuid
    }
}

/// Ids are UUID strings in TypeScript (`packages/api-types`).
impl<T> ts_rs::TS for Id<T> {
    type WithoutGenerics = Id<ts_rs::Dummy>;
    type OptionInnerType = Self;

    fn name(_: &ts_rs::Config) -> String {
        "string".to_owned()
    }

    fn inline(cfg: &ts_rs::Config) -> String {
        Self::name(cfg)
    }
}

impl<T> Serialize for Id<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.uuid.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Id<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Uuid::deserialize(deserializer).map(Self::from_uuid)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use proptest::prelude::*;

    use super::*;

    enum Item {}
    type ItemId = Id<Item>;

    #[test]
    fn new_ids_are_version_7_and_unique() {
        let a = ItemId::new();
        let b = ItemId::new();
        assert_eq!(a.as_uuid().get_version_num(), 7);
        assert_ne!(a, b);
    }

    #[test]
    fn serializes_as_plain_string() {
        let id: ItemId = "01936d3c-7a8e-7cc1-9a3b-2f4e5d6c7b8a".parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, r#""01936d3c-7a8e-7cc1-9a3b-2f4e5d6c7b8a""#);
        assert_eq!(serde_json::from_str::<ItemId>(&json).unwrap(), id);
    }

    #[test]
    fn rejects_malformed_strings() {
        assert!("not-a-uuid".parse::<ItemId>().is_err());
        assert!(serde_json::from_str::<ItemId>(r#""123""#).is_err());
    }

    #[test]
    fn new_at_embeds_the_given_timestamp() {
        let at = Utc.with_ymd_and_hms(2026, 11, 21, 12, 0, 0).unwrap();
        let id = ItemId::new_at(at);
        let (secs, _) = id.as_uuid().get_timestamp().unwrap().to_unix();
        assert_eq!(i64::try_from(secs).unwrap(), at.timestamp());
    }

    proptest! {
        #[test]
        fn display_parse_roundtrip(bytes in any::<[u8; 16]>()) {
            let id = ItemId::from_uuid(Uuid::from_bytes(bytes));
            prop_assert_eq!(id.to_string().parse::<ItemId>().unwrap(), id);
        }

        /// UUIDv7 ids sort by creation time (millisecond resolution).
        #[test]
        fn ids_sort_by_timestamp(a in 0i64..4_000_000_000_000, b in 0i64..4_000_000_000_000) {
            prop_assume!(a != b);
            let (early, late) = (a.min(b), a.max(b));
            let early = ItemId::new_at(Utc.timestamp_millis_opt(early).unwrap());
            let late = ItemId::new_at(Utc.timestamp_millis_opt(late).unwrap());
            prop_assert!(early < late);
        }
    }
}
