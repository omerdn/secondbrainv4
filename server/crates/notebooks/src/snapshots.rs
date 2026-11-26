//! Page snapshots, compaction and plain text (ARCHITECTURE §9, PRD Y3, T2.5.3).
//!
//! A page is its latest snapshot plus every update still stored. [`compact`] folds them into a
//! new snapshot and deletes exactly the updates it read, in one transaction, so a retry or a
//! repeated job changes nothing and an update that commits during compaction stays stored for the
//! next one. Compaction runs every [`COMPACT_EVERY`] updates of a room and on a schedule.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use yrs::types::text::YChange;
use yrs::types::xml::{XmlFragment, XmlOut};
use yrs::updates::decoder::Decode;
use yrs::{Any, Doc, GetString, Out, ReadTxn, StateVector, Text, Transact, Update};

use crate::repo;

/// Updates of one room after which it is compacted (ARCHITECTURE §9).
pub const COMPACT_EVERY: u32 = 200;
/// Cron of the sweep that compacts every page with stored updates (every 10 minutes).
pub const COMPACT_CRON: &str = "0 */10 * * * *";
/// Pages one sweep compacts; the next sweep continues.
const SWEEP_LIMIT: i64 = 500;
/// Longest plain text kept for search.
const PLAIN_TEXT_MAX_CHARS: usize = 200_000;
/// Root of the TipTap document (y-prosemirror); `content` is a plain text root used in tests.
const PROSEMIRROR_ROOT: &str = "default";
const TEXT_ROOT: &str = "content";

/// Compacts one page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactPage {
    pub page_id: Uuid,
}

impl jobs::Job for CompactPage {
    const KIND: &'static str = "notebooks.compact_page";
}

/// Compacts every page with stored updates (scheduled).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactPendingPages {}

impl jobs::Job for CompactPendingPages {
    const KIND: &'static str = "notebooks.compact_pending_pages";
}

/// Errors of rebuilding a page from storage.
#[derive(Debug, thiserror::Error)]
pub enum PageError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("stored page data does not decode: {0}")]
    Decode(String),
}

/// A document holding `snapshot` and then `updates`.
///
/// # Errors
/// [`PageError::Decode`] if stored data does not apply; dropping it would lose data.
pub fn rebuild(snapshot: Option<&[u8]>, updates: &[Vec<u8>]) -> Result<Doc, PageError> {
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        for bytes in snapshot
            .into_iter()
            .chain(updates.iter().map(Vec::as_slice))
        {
            let update =
                Update::decode_v1(bytes).map_err(|err| PageError::Decode(err.to_string()))?;
            txn.apply_update(update)
                .map_err(|err| PageError::Decode(err.to_string()))?;
        }
    }
    Ok(doc)
}

/// The whole document as one update.
#[must_use]
pub fn encode(doc: &Doc) -> Vec<u8> {
    doc.transact()
        .encode_state_as_update_v1(&StateVector::default())
}

/// The page as stored: latest snapshot plus remaining updates.
///
/// # Errors
/// [`PageError`].
pub async fn load(pool: &PgPool, page: Uuid) -> Result<Doc, PageError> {
    let snapshot = repo::latest_snapshot(pool, page).await?;
    let updates = repo::page_updates(pool, page).await?;
    rebuild(
        snapshot.as_ref().map(|(_, state)| state.as_slice()),
        &updates,
    )
}

fn push_text(out: &mut String, txn: &impl ReadTxn, node: &XmlOut) {
    match node {
        XmlOut::Text(text) => {
            for chunk in text.diff(txn, YChange::identity) {
                if let Out::Any(Any::String(s)) = chunk.insert {
                    out.push_str(&s);
                }
            }
        }
        XmlOut::Element(element) => {
            for child in element.children(txn) {
                push_text(out, txn, &child);
            }
            // Every element ends a block: paragraphs, headings, list items, code, quotes.
            out.push('\n');
        }
        XmlOut::Fragment(fragment) => {
            for child in fragment.children(txn) {
                push_text(out, txn, &child);
            }
        }
    }
}

/// The page's text without formatting, one block per line, for search (ARCHITECTURE §12).
#[must_use]
pub fn plain_text(doc: &Doc) -> String {
    let txn = doc.transact();
    let mut raw = String::new();
    if let Some(root) = txn.get_xml_fragment(PROSEMIRROR_ROOT) {
        for child in root.children(&txn) {
            push_text(&mut raw, &txn, &child);
        }
    }
    if let Some(text) = txn.get_text(TEXT_ROOT) {
        raw.push_str(&text.get_string(&txn));
    }
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines
        .join("\n")
        .chars()
        .take(PLAIN_TEXT_MAX_CHARS)
        .collect()
}

/// What one compaction did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compacted {
    /// Nothing was stored since the last snapshot.
    Nothing,
    /// A new snapshot `version` holds `updates` more updates.
    Snapshot { version: i32, updates: usize },
}

/// Folds the page's stored updates into a new snapshot (one transaction; see module docs).
///
/// # Errors
/// [`PageError`]; nothing changes then.
pub async fn compact(
    pool: &PgPool,
    page: Uuid,
    now: DateTime<Utc>,
) -> Result<Compacted, PageError> {
    let mut tx = pool.begin().await?;
    repo::lock_page_compaction(&mut tx, page).await?;
    let updates = repo::page_updates_with_ids(&mut *tx, page).await?;
    if updates.is_empty() {
        return Ok(Compacted::Nothing);
    }
    let snapshot = repo::latest_snapshot(&mut *tx, page).await?;
    let (ids, bytes): (Vec<i64>, Vec<Vec<u8>>) = updates.into_iter().unzip();
    let doc = rebuild(snapshot.as_ref().map(|(_, state)| state.as_slice()), &bytes)?;
    let version = snapshot.map_or(1, |(version, _)| version + 1);
    repo::insert_snapshot(
        &mut tx,
        (page, version),
        &encode(&doc),
        &plain_text(&doc),
        now,
    )
    .await?;
    repo::delete_updates(&mut tx, page, &ids).await?;
    tx.commit().await?;
    Ok(Compacted::Snapshot {
        version,
        updates: ids.len(),
    })
}

/// Compacts every page with stored updates, at most a few hundred per call.
///
/// # Errors
/// [`PageError`] of the first page that fails; the others stay for the next sweep.
pub async fn compact_pending(pool: &PgPool, now: DateTime<Utc>) -> Result<usize, PageError> {
    let pages = repo::pages_with_updates(pool, SWEEP_LIMIT).await?;
    for page in &pages {
        compact(pool, *page, now).await?;
    }
    Ok(pages.len())
}

/// Registers the compaction jobs and the 10-minute sweep (call from `app::run`).
///
/// # Errors
/// [`jobs::ScheduleError`] for a bad schedule.
pub fn register(
    worker: &mut jobs::Worker,
    scheduler: &mut jobs::Scheduler,
    pool: PgPool,
    clock: kernel::SharedClock,
) -> Result<(), jobs::ScheduleError> {
    {
        let pool = pool.clone();
        let clock = clock.clone();
        worker.register(move |job: CompactPage, _| {
            let pool = pool.clone();
            let now = clock.now();
            async move {
                compact(&pool, job.page_id, now)
                    .await
                    .map_err(|err| jobs::HandlerError::from(Box::new(err) as Box<_>))?;
                Ok(())
            }
        });
    }
    worker.register(move |_: CompactPendingPages, _| {
        let pool = pool.clone();
        let now = clock.now();
        async move {
            compact_pending(&pool, now)
                .await
                .map_err(|err| jobs::HandlerError::from(Box::new(err) as Box<_>))?;
            Ok(())
        }
    });
    scheduler.add("notebooks-compact", COMPACT_CRON, CompactPendingPages {})?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use yrs::{XmlElementPrelim, XmlTextPrelim};

    use super::*;

    #[test]
    fn plain_text_reads_blocks_and_drops_formatting() {
        let doc = Doc::new();
        let root = doc.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
        {
            let mut txn = doc.transact_mut();
            let heading = root.push_back(&mut txn, XmlElementPrelim::empty("heading"));
            heading.push_back(&mut txn, XmlTextPrelim::new("Başlık"));
            let paragraph = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            let text = paragraph.push_back(&mut txn, XmlTextPrelim::new("kalın ve düz"));
            text.format(
                &mut txn,
                0,
                5,
                HashMap::from([("bold".into(), Any::Bool(true))]),
            );
            let list = root.push_back(&mut txn, XmlElementPrelim::empty("bulletList"));
            let item = list.push_back(&mut txn, XmlElementPrelim::empty("listItem"));
            let inner = item.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            inner.push_back(&mut txn, XmlTextPrelim::new("madde"));
            root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        }
        assert_eq!(plain_text(&doc), "Başlık\nkalın ve düz\nmadde");
    }

    #[test]
    fn plain_text_is_bounded() {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_ROOT);
        text.insert(
            &mut doc.transact_mut(),
            0,
            &"a".repeat(PLAIN_TEXT_MAX_CHARS + 10),
        );
        assert_eq!(plain_text(&doc).chars().count(), PLAIN_TEXT_MAX_CHARS);
        assert_eq!(plain_text(&Doc::new()), "");
    }

    #[test]
    fn rebuilding_from_a_snapshot_and_updates_is_the_same_document() {
        let doc = Doc::new();
        let text = doc.get_or_insert_text(TEXT_ROOT);
        let mut updates = Vec::new();
        for chunk in ["bir ", "iki ", "üç"] {
            let before = doc.transact().state_vector();
            let len = text.len(&doc.transact());
            text.insert(&mut doc.transact_mut(), len, chunk);
            updates.push(doc.transact().encode_state_as_update_v1(&before));
        }
        let all = rebuild(None, &updates).unwrap();
        let snapshot = encode(&rebuild(None, &updates[..2]).unwrap());
        // A snapshot plus the rest, and a snapshot plus updates it already holds.
        for doc in [
            rebuild(Some(&snapshot), &updates[2..]).unwrap(),
            rebuild(Some(&snapshot), &updates).unwrap(),
        ] {
            assert_eq!(plain_text(&doc), plain_text(&all));
            assert_eq!(plain_text(&doc), "bir iki üç");
        }
        assert!(matches!(
            rebuild(None, &[vec![1, 2, 3]]),
            Err(PageError::Decode(_))
        ));
    }

    use std::collections::HashMap;
}
