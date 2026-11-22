//! `tracking`: Tracking engine: trackers, entries and `ProgressProvider` (ADR-0006).
//!
//! See ARCHITECTURE §5.5 and PRD T1–T5. Storage lives in [`repo`]; aggregation (T1.3.2), progress
//! (T1.3.3) and the API (T1.3.4) build on it.

mod model;
pub mod repo;

pub use model::{
    Aggregation, Direction, NewEntry, NewTracker, OwnerKind, Period, Tracker, TrackerEntry,
};
