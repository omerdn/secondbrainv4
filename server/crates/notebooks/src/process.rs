//! Process pages (PRD S1–S3, T3, ARCHITECTURE §9, T2.5.5).
//!
//! An item gets its process page with its first note: when someone starts writing about it
//! (`POST /items/{id}/process-page`) or when an entry is added with a note. The page lives in the
//! default notebook of the item's owner (S2) and is linked to the item, so everyone who can see
//! the item can see and edit it (V2, V3).
//!
//! An entry note becomes a dated paragraph that the server appends with yrs (T3). The paragraph
//! carries the entry id as `blockId` and the entry's day as `date`; the entry stores the block id
//! as `note_block_id`, and from then on its note *is* that line: an edit on the page is the edit
//! of the note. Appending is idempotent (one block per entry), so a retried event or the sweep
//! never adds a second line.

use std::collections::{HashMap, HashSet};

use access::{AccessEngine, PermissionService, Perms};
use chrono::NaiveDate;
use identity::IdentityService;
use identity::model::UserId;
use items::{ItemsAccessContext, ItemsService, Owner};
use kernel::AppError;
use serde::Serialize;
use sqlx::PgPool;
use tracking::{EntryNoted, NoteSource, NotesFuture, TrackerEntries};
use ts_rs::TS;
use uuid::Uuid;
use yrs::types::xml::{Xml, XmlFragment, XmlOut};
use yrs::{
    Any, Doc, GetString, Out, ReadTxn, Transact, XmlElementPrelim, XmlElementRef, XmlTextPrelim,
};

use crate::collab::Collab;
use crate::model::{Page, default_title};
use crate::repo;
use crate::service::NotebooksService;
use crate::snapshots::{self, PROSEMIRROR_ROOT};

/// Attribute holding a server-written line's block id (the entry id).
pub const BLOCK_ID: &str = "blockId";
/// Attribute holding a server-written line's day (`YYYY-MM-DD`).
pub const LINE_DATE: &str = "date";
/// Cron of the sweep that moves notes not yet on a process page (every 10 minutes).
pub const LINK_NOTES_CRON: &str = "30 */10 * * * *";

/// The latest line of a process page (S3): "Barre'da kaldım · 3 gün önce".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct StatusLine {
    pub text: String,
    /// The day of the entry the line came from; none for a line someone wrote.
    #[ts(type = "string | null")]
    pub date: Option<NaiveDate>,
}

/// The latest line of one item's process page, for cards (S3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ItemStatusLine {
    pub item_id: Uuid,
    pub line: StatusLine,
}

/// Items one status line request may ask for.
pub const MAX_STATUS_LINE_ITEMS: usize = 100;

/// An item's process page with its latest line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ProcessPage {
    pub page: Page,
    pub latest: Option<StatusLine>,
}

/// What the server writes before a note: the day, so the page reads as a log.
#[must_use]
pub fn line_prefix(date: NaiveDate) -> String {
    format!("{} · ", date.format("%d.%m.%Y"))
}

fn text_of(txn: &impl ReadTxn, element: &XmlElementRef) -> String {
    let mut out = String::new();
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => out.push_str(&text.get_string(txn)),
            XmlOut::Element(inner) => out.push_str(&text_of(txn, &inner)),
            XmlOut::Fragment(_) => {}
        }
    }
    out
}

fn string_attribute(txn: &impl ReadTxn, element: &XmlElementRef, name: &str) -> Option<String> {
    match element.get_attribute(txn, name)? {
        Out::Any(Any::String(value)) => Some(value.to_string()),
        _ => None,
    }
}

/// A line's text without the prefix the server wrote, if it is still there.
fn note_text(txn: &impl ReadTxn, element: &XmlElementRef) -> String {
    let text = text_of(txn, element);
    let date = string_attribute(txn, element, LINE_DATE).and_then(|d| d.parse().ok());
    let note = match date {
        Some(date) => text
            .strip_prefix(&line_prefix(date))
            .unwrap_or(&text)
            .to_owned(),
        None => text,
    };
    note.trim().to_owned()
}

/// Calls `visit` for every element of the page, depth first, in document order.
fn walk(doc: &Doc, mut visit: impl FnMut(&yrs::Transaction<'_>, &XmlElementRef)) {
    fn go(
        txn: &yrs::Transaction<'_>,
        node: &impl XmlFragment,
        visit: &mut impl FnMut(&yrs::Transaction<'_>, &XmlElementRef),
    ) {
        for child in node.children(txn) {
            if let XmlOut::Element(element) = child {
                visit(txn, &element);
                go(txn, &element, visit);
            }
        }
    }
    let txn = doc.transact();
    if let Some(root) = txn.get_xml_fragment(PROSEMIRROR_ROOT) {
        go(&txn, &root, &mut visit);
    }
}

/// The current text of the lines `blocks` (T3). A line deleted from the page is missing.
#[must_use]
pub fn lines(doc: &Doc, blocks: &[Uuid]) -> HashMap<Uuid, String> {
    let wanted: HashSet<String> = blocks.iter().map(Uuid::to_string).collect();
    let mut found = HashMap::new();
    walk(doc, |txn, element| {
        if let Some(id) = string_attribute(txn, element, BLOCK_ID)
            && wanted.contains(&id)
            && let Ok(block) = id.parse()
        {
            found
                .entry(block)
                .or_insert_with(|| note_text(txn, element));
        }
    });
    found
}

/// The page's last block with text (S3).
#[must_use]
pub fn latest_line(doc: &Doc) -> Option<StatusLine> {
    let txn = doc.transact();
    let root = txn.get_xml_fragment(PROSEMIRROR_ROOT)?;
    let blocks: Vec<XmlElementRef> = root
        .children(&txn)
        .filter_map(|child| match child {
            XmlOut::Element(element) => Some(element),
            _ => None,
        })
        .collect();
    blocks.iter().rev().find_map(|element| {
        let text = note_text(&txn, element);
        (!text.is_empty()).then(|| StatusLine {
            text,
            date: string_attribute(&txn, element, LINE_DATE).and_then(|d| d.parse().ok()),
        })
    })
}

/// The update that appends the note of entry `block` as a dated paragraph; empty if the page
/// already has that line (T3).
#[must_use]
pub fn note_line_update(doc: &Doc, block: Uuid, date: NaiveDate, note: &str) -> Vec<u8> {
    let id = block.to_string();
    let mut present = false;
    walk(doc, |txn, element| {
        present |= string_attribute(txn, element, BLOCK_ID).as_deref() == Some(id.as_str());
    });
    if present {
        return Vec::new();
    }
    let scratch = snapshots::scratch_copy(doc);
    let before = scratch.transact().state_vector();
    let root = scratch.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
    {
        let mut txn = scratch.transact_mut();
        let paragraph = root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.insert_attribute(&mut txn, BLOCK_ID, id);
        paragraph.insert_attribute(&mut txn, LINE_DATE, date.to_string());
        paragraph.push_back(
            &mut txn,
            XmlTextPrelim::new(format!("{}{note}", line_prefix(date))),
        );
    }
    scratch.transact().encode_state_as_update_v1(&before)
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Process pages; cheap to clone.
#[derive(Clone)]
pub struct ProcessPages {
    pool: PgPool,
    notebooks: NotebooksService,
    items: ItemsService,
    identity: IdentityService,
    collab: Collab,
    permissions: PermissionService<ItemsAccessContext>,
}

impl std::fmt::Debug for ProcessPages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessPages").finish_non_exhaustive()
    }
}

impl ProcessPages {
    #[must_use]
    pub fn new(
        pool: PgPool,
        notebooks: NotebooksService,
        (items, identity): (ItemsService, IdentityService),
        collab: Collab,
        access: &AccessEngine<ItemsAccessContext>,
    ) -> Self {
        Self {
            pool,
            notebooks,
            items,
            identity,
            collab,
            permissions: access.permissions(),
        }
    }

    /// The process page of `item`, created if it has none (S1, S2); `actor` must see the item.
    async fn ensure(&self, actor: Uuid, item_id: Uuid) -> Result<Uuid, AppError> {
        if let Some(page) = repo::process_page_of(&self.pool, item_id)
            .await
            .map_err(internal)?
        {
            return Ok(page);
        }
        let item = self.items.get(actor, item_id).await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        if !items::link_allowed("page", &item.kind) {
            return Err(AppError::Conflict(
                "this kind of item has no process page".into(),
            ));
        }
        let Owner::User { user_id: owner } = item.owner else {
            return Err(AppError::Conflict(
                "team items have no process page yet".into(),
            ));
        };
        let locale = self
            .identity
            .current_user(UserId::from_uuid(owner))
            .await?
            .locale;
        let notebook = self
            .notebooks
            .ensure_default(owner, default_title(&locale))
            .await?;
        let page = self
            .notebooks
            .create_process_page((actor, owner), notebook, item_id, item.title)
            .await?;
        // V2: whoever sees the item sees the page. The owner may link their own page; linking
        // again is a no-op, so a retry after a failure here completes it.
        self.items.link(owner, page, item_id).await?;
        Ok(page)
    }

    async fn view(&self, actor: Uuid, page: Uuid) -> Result<ProcessPage, AppError> {
        let page = self.notebooks.page(actor, page).await?;
        let doc = snapshots::load(&self.pool, page.item.id)
            .await
            .map_err(internal)?;
        Ok(ProcessPage {
            page,
            latest: latest_line(&doc),
        })
    }

    /// An item's process page and its latest line (VIEW on the item, S3).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the item is not visible or has no process page, or
    /// [`AppError::Internal`].
    pub async fn get(&self, actor: Uuid, item: Uuid) -> Result<ProcessPage, AppError> {
        self.permissions
            .require(&self.pool, actor, item, Perms::VIEW)
            .await?;
        let page = repo::process_page_of(&self.pool, item)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        self.view(actor, page).await
    }

    /// The latest lines of `items`' process pages, for cards (S3). Only items and pages the
    /// caller can see count; anything else is left out as if it had no line.
    ///
    /// # Errors
    /// [`AppError::Validation`] for too many items, or [`AppError::Internal`].
    pub async fn latest_lines(
        &self,
        actor: Uuid,
        items: &[Uuid],
    ) -> Result<Vec<ItemStatusLine>, AppError> {
        if items.len() > MAX_STATUS_LINE_ITEMS {
            return Err(kernel::ValidationErrors::from(kernel::FieldError::new(
                "ids",
                kernel::FieldErrorCode::TooLong,
                format!("at most {MAX_STATUS_LINE_ITEMS} items"),
            ))
            .into());
        }
        let visible: HashSet<Uuid> = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?
            .into_iter()
            .collect();
        let asked: Vec<Uuid> = items
            .iter()
            .copied()
            .filter(|id| visible.contains(id))
            .collect();
        let pages = repo::process_pages_of(&self.pool, &asked)
            .await
            .map_err(internal)?;
        let mut lines = Vec::new();
        for (item_id, page) in pages {
            // The page must be visible too: an unlinked page no longer follows its item (V2).
            if !visible.contains(&page) {
                continue;
            }
            let doc = snapshots::load(&self.pool, page).await.map_err(internal)?;
            if let Some(line) = latest_line(&doc) {
                lines.push(ItemStatusLine { item_id, line });
            }
        }
        Ok(lines)
    }

    /// The item's process page, created when someone starts writing about it (EDIT, S1).
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`] without EDIT, [`AppError::Conflict`] for
    /// an item in the trash, a team item or a kind without process pages, or
    /// [`AppError::Internal`].
    pub async fn open(&self, actor: Uuid, item: Uuid) -> Result<ProcessPage, AppError> {
        self.permissions
            .require(&self.pool, actor, item, Perms::EDIT)
            .await?;
        let page = self.ensure(actor, item).await?;
        self.view(actor, page).await
    }

    /// Puts an entry's note on the item's process page as a dated line (S1, T3) and links the
    /// entry to it. Repeating it changes nothing.
    ///
    /// # Errors
    /// [`AppError`] of creating the page or writing the line; the note stays on the entry and
    /// the sweep tries again.
    pub async fn add_note(
        &self,
        event: &EntryNoted,
        entries: &TrackerEntries,
    ) -> Result<(), AppError> {
        let page = self.ensure(event.user_id, event.item_id).await?;
        self.collab
            .change(page, event.user_id, |doc| {
                note_line_update(doc, event.entry_id, event.local_date, &event.note)
            })
            .await?;
        entries.link_note(event.entry_id, event.entry_id).await
    }

    /// Moves notes that did not reach a process page yet (older entries, failed attempts).
    /// Returns how many moved.
    ///
    /// # Errors
    /// [`AppError::Internal`] for listing them; a failing note is logged and skipped.
    pub async fn add_pending_notes(&self, entries: &TrackerEntries) -> Result<usize, AppError> {
        let mut moved = 0;
        for event in entries.unlinked_notes().await? {
            match self.add_note(&event, entries).await {
                Ok(()) => moved += 1,
                Err(err) => tracing::warn!(error = %err, "entry note not moved to its page"),
            }
        }
        Ok(moved)
    }
}

impl NoteSource for ProcessPages {
    fn notes(&self, item: Uuid, blocks: Vec<Uuid>) -> NotesFuture<'_> {
        Box::pin(async move {
            let Some(page) = repo::process_page_of(&self.pool, item)
                .await
                .map_err(internal)?
            else {
                return Ok(HashMap::new());
            };
            let doc = snapshots::load(&self.pool, page).await.map_err(internal)?;
            Ok(lines(&doc, &blocks)
                .into_iter()
                .filter(|(_, text)| !text.is_empty())
                .collect())
        })
    }
}

/// Moves new entry notes to process pages (outbox) and runs the sweep for the rest (call from
/// `app::run`).
///
/// # Errors
/// [`jobs::ScheduleError`] for a bad schedule.
pub fn register(
    dispatcher: &mut db::outbox::Dispatcher,
    (worker, scheduler): (&mut jobs::Worker, &mut jobs::Scheduler),
    process: ProcessPages,
    entries: TrackerEntries,
) -> Result<(), jobs::ScheduleError> {
    {
        let process = process.clone();
        let entries = entries.clone();
        dispatcher.subscribe(
            "notebooks.entry-note-to-process-page",
            move |event: EntryNoted, _| {
                let process = process.clone();
                let entries = entries.clone();
                async move {
                    process
                        .add_note(&event, &entries)
                        .await
                        .map_err(|err| Box::new(err) as db::outbox::HandlerError)
                }
            },
        );
    }
    worker.register(move |_: LinkPendingNotes, _| {
        let process = process.clone();
        let entries = entries.clone();
        async move {
            process
                .add_pending_notes(&entries)
                .await
                .map_err(|err| jobs::HandlerError::from(Box::new(err) as Box<_>))?;
            Ok(())
        }
    });
    scheduler.add("notebooks-link-notes", LINK_NOTES_CRON, LinkPendingNotes {})?;
    Ok(())
}

/// Moves entry notes that are not on a process page yet (scheduled).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LinkPendingNotes {}

impl jobs::Job for LinkPendingNotes {
    const KIND: &'static str = "notebooks.link_pending_notes";
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use yrs::updates::decoder::Decode;
    use yrs::{Text, Update, XmlTextRef};

    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 26).unwrap()
    }

    fn apply(doc: &Doc, update: &[u8]) {
        doc.transact_mut()
            .apply_update(Update::decode_v1(update).unwrap())
            .unwrap();
    }

    #[test]
    fn t3_a_note_becomes_one_dated_line() {
        let doc = Doc::new();
        let entry = Uuid::now_v7();
        apply(
            &doc,
            &note_line_update(&doc, entry, day(), "Barre'da kaldım"),
        );
        assert_eq!(snapshots::plain_text(&doc), "26.11.2026 · Barre'da kaldım");
        assert_eq!(lines(&doc, &[entry])[&entry], "Barre'da kaldım");
        // Once per entry.
        assert!(note_line_update(&doc, entry, day(), "Barre'da kaldım").is_empty());
        assert_eq!(
            latest_line(&doc),
            Some(StatusLine {
                text: "Barre'da kaldım".into(),
                date: Some(day()),
            })
        );
    }

    #[test]
    fn t3_editing_the_line_edits_the_note() {
        let doc = Doc::new();
        let entry = Uuid::now_v7();
        apply(&doc, &note_line_update(&doc, entry, day(), "eski"));
        let root = doc.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
        {
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(line)) = root.get(&txn, 0) else {
                panic!("no line")
            };
            let Some(XmlOut::Text(text)) = line.get(&txn, 0) else {
                panic!("no text")
            };
            let text: XmlTextRef = text;
            let prefix = line_prefix(day()).len();
            text.remove_range(&mut txn, u32::try_from(prefix).unwrap(), 4);
            text.insert(&mut txn, u32::try_from(prefix).unwrap(), "yeni");
        }
        assert_eq!(lines(&doc, &[entry])[&entry], "yeni");
        // Also when the date is rewritten by hand.
        {
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(line)) = root.get(&txn, 0) else {
                panic!("no line")
            };
            let Some(XmlOut::Text(text)) = line.get(&txn, 0) else {
                panic!("no text")
            };
            text.remove_range(&mut txn, 0, 2);
        }
        assert_eq!(lines(&doc, &[entry])[&entry], ".11.2026 · yeni");
        // A deleted line is a deleted note.
        {
            let mut txn = doc.transact_mut();
            root.remove_range(&mut txn, 0, 1);
        }
        assert!(lines(&doc, &[entry]).is_empty());
        assert_eq!(latest_line(&doc), None);
    }

    #[test]
    fn s3_the_latest_line_is_the_last_block_with_text() {
        let doc = Doc::new();
        apply(
            &doc,
            &note_line_update(&doc, Uuid::now_v7(), day(), "kayıt"),
        );
        let root = doc.get_or_insert_xml_fragment(PROSEMIRROR_ROOT);
        {
            let mut txn = doc.transact_mut();
            root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"))
                .push_back(&mut txn, XmlTextPrelim::new("elle yazılan"));
            root.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        }
        assert_eq!(
            latest_line(&doc),
            Some(StatusLine {
                text: "elle yazılan".into(),
                date: None,
            })
        );
        assert_eq!(latest_line(&Doc::new()), None);
    }
}
