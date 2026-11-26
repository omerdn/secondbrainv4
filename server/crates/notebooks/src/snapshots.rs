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
use yrs::types::xml::{Xml, XmlFragment, XmlOut};
use yrs::types::{Attrs, ToJson};
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Doc, GetString, Out, ReadTxn, StateVector, Text, Transact, TransactionMut, Update,
    XmlElementPrelim, XmlTextPrelim, XmlTextRef,
};

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

/// Copies the formatted text of `src` to the end of `dst`.
fn copy_text<S: Text, D: Text>(
    src: &S,
    src_txn: &impl ReadTxn,
    dst: &D,
    txn: &mut TransactionMut<'_>,
) {
    for chunk in src.diff(src_txn, YChange::identity) {
        let attrs: Attrs = chunk.attributes.map(|attrs| *attrs).unwrap_or_default();
        let at = dst.len(txn);
        match chunk.insert {
            Out::Any(Any::String(s)) => dst.insert_with_attributes(txn, at, &s, attrs),
            other => {
                dst.insert_embed_with_attributes(txn, at, other.to_json(src_txn), attrs);
            }
        }
    }
}

/// Copies the attributes of `src` onto `dst`.
fn copy_attributes(
    src: &impl Xml,
    src_txn: &impl ReadTxn,
    dst: &impl Xml,
    txn: &mut TransactionMut<'_>,
) {
    for (key, value) in src.attributes(src_txn) {
        dst.insert_attribute(txn, key, value.to_json(src_txn));
    }
}

/// Appends deep copies of the children of `src` to `dst` (another document).
fn copy_children(
    src: &impl XmlFragment,
    src_txn: &impl ReadTxn,
    dst: &impl XmlFragment,
    txn: &mut TransactionMut<'_>,
) {
    for child in src.children(src_txn) {
        match child {
            XmlOut::Element(element) => {
                let copy = dst.push_back(txn, XmlElementPrelim::empty(element.tag().as_ref()));
                copy_attributes(&element, src_txn, &copy, txn);
                copy_children(&element, src_txn, &copy, txn);
            }
            XmlOut::Text(text) => {
                let copy: XmlTextRef = dst.push_back(txn, XmlTextPrelim::new(""));
                copy_attributes(&text, src_txn, &copy, txn);
                copy_text(&text, src_txn, &copy, txn);
            }
            // y-prosemirror never nests fragments; keep their content in place anyway.
            XmlOut::Fragment(fragment) => copy_children(&fragment, src_txn, dst, txn),
        }
    }
}

/// The update that turns `current` into the content of `target`, applied on top of `current`
/// (PRD Y3, ARCHITECTURE §9): restoring a version is a new change, so history stays and
/// concurrent edits merge. Empty if both already hold the same content.
#[must_use]
pub fn revert_update(current: &Doc, target: &Doc) -> Vec<u8> {
    if encode_content(current) == encode_content(target) {
        return Vec::new();
    }
    // A copy with its own client id, so the change never collides with an editor's.
    let scratch = Doc::new();
    {
        let mut txn = scratch.transact_mut();
        if let Ok(update) = Update::decode_v1(&encode(current)) {
            let _ = txn.apply_update(update);
        }
    }
    let before = scratch.transact().state_vector();
    let fragment = scratch.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
    let text = scratch.get_or_insert_text(TEXT_ROOT);
    {
        let target_txn = target.transact();
        let mut txn = scratch.transact_mut();
        let len = fragment.len(&txn);
        fragment.remove_range(&mut txn, 0, len);
        if let Some(src) = target_txn.get_xml_fragment(PROSEMIRROR_ROOT) {
            copy_children(&src, &target_txn, &fragment, &mut txn);
        }
        let len = text.len(&txn);
        text.remove_range(&mut txn, 0, len);
        if let Some(src) = target_txn.get_text(TEXT_ROOT) {
            copy_text(&src, &target_txn, &text, &mut txn);
        }
    }
    scratch.transact().encode_state_as_update_v1(&before)
}

/// The content of `doc` as comparable text: its structure, attributes and formatting.
fn encode_content(doc: &Doc) -> String {
    let txn = doc.transact();
    let mut out = String::new();
    if let Some(root) = txn.get_xml_fragment(PROSEMIRROR_ROOT) {
        out.push_str(&root.get_string(&txn));
    }
    out.push('\u{0}');
    if let Some(text) = txn.get_text(TEXT_ROOT) {
        for chunk in text.diff(&txn, YChange::identity) {
            out.push_str(&format!("{:?}{:?}", chunk.insert, chunk.attributes));
        }
    }
    out
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

    /// A TipTap-like document: a heading with a level and a paragraph with bold text.
    fn rich(heading: &str, body: &str) -> Doc {
        let doc = Doc::new();
        let root = doc.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
        {
            let mut txn = doc.transact_mut();
            let h = root.push_back(&mut txn, XmlElementPrelim::empty("heading"));
            h.insert_attribute(&mut txn, "level", Any::from(2));
            h.push_back(&mut txn, XmlTextPrelim::new(heading));
            let p = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            let text = p.push_back(&mut txn, XmlTextPrelim::new(body));
            text.format(
                &mut txn,
                0,
                1,
                HashMap::from([("bold".into(), Any::Bool(true))]),
            );
        }
        doc
    }

    fn apply(doc: &Doc, update: &[u8]) {
        doc.transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }

    fn copy(doc: &Doc) -> Doc {
        rebuild(Some(&encode(doc)), &[]).unwrap()
    }

    #[test]
    fn y3_reverting_brings_back_structure_attributes_and_formatting() {
        let old = rich("Eski", "kalın");
        let current = copy(&old);
        // Edited since: more text and another block.
        {
            let root = current.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
            let mut txn = current.transact_mut();
            root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"))
                .push_back(&mut txn, XmlTextPrelim::new("yeni"));
        }
        assert_ne!(encode_content(&current), encode_content(&old));
        let update = revert_update(&current, &old);
        apply(&current, &update);
        assert_eq!(encode_content(&current), encode_content(&old));
        assert_eq!(plain_text(&current), "Eski\nkalın");
        // Nothing changes when the content is already the same.
        assert!(revert_update(&current, &old).is_empty());
    }

    #[test]
    fn y3_a_revert_merges_with_a_concurrent_edit() {
        let old = Doc::new();
        let text = old.get_or_insert_text(TEXT_ROOT);
        text.insert(&mut old.transact_mut(), 0, "bir");
        let server = copy(&old);
        let text = server.get_or_insert_text(TEXT_ROOT);
        let len = text.len(&server.transact());
        text.insert(&mut server.transact_mut(), len, " iki");
        // An editor types while the revert is being made.
        let editor = copy(&server);
        let before = editor.transact().state_vector();
        let etext = editor.get_or_insert_text(TEXT_ROOT);
        let len = etext.len(&editor.transact());
        etext.insert(&mut editor.transact_mut(), len, " üç");
        let typed = editor.transact().encode_state_as_update_v1(&before);

        let revert = revert_update(&server, &old);
        apply(&server, &typed);
        apply(&server, &revert);
        apply(&editor, &revert);
        // Both converge; the old content is back and the concurrent typing is kept (no data
        // loss). Where the typing lands relative to the restored text is up to the CRDT.
        let result = plain_text(&server);
        assert_eq!(result, plain_text(&editor));
        assert!(result.contains("bir") && result.contains("üç"), "{result}");
        assert!(!result.contains("iki"), "{result}");
    }

    use std::collections::HashMap;
}
