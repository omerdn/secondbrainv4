//! Collecting and rendering an export (HS3). Everything is read through the owning modules'
//! services **as the user**, so only what they may see is included and hidden links never
//! appear (V9): a link, a member or a linked item the user cannot see is left out by the same
//! code that answers the API.

use std::fmt::Write as _;

use chrono::{DateTime, Days, NaiveDate, Utc};
use identity::model::CurrentUser;
use items::{Item, ListState};
use kernel::{AppError, PageRequest};
use serde::Serialize;
use uuid::Uuid;

/// Routine days included per routine (the services' own maximum range).
pub const ROUTINE_DAYS: u64 = 399;

/// The services an export reads from, all permission-checked.
#[derive(Clone)]
pub struct Sources {
    pub identity: identity::IdentityService,
    pub items: items::ItemsService,
    pub tasks: tasks::TasksService,
    pub routines: routines::RoutinesService,
    pub goals: goals::GoalsService,
    pub trackers: tracking::TrackerEntries,
    pub notebooks: notebooks::NotebooksService,
    pub sessions: sessions::WorkSessions,
}

/// One item with what its module knows about it.
#[derive(Debug, Clone, Serialize)]
pub struct ExportedItem {
    pub item: Item,
    pub tags: Vec<items::Tag>,
    pub links: Vec<items::ItemLink>,
    /// The module's view (task, routine, goal, tracker); `null` for other kinds.
    pub details: serde_json::Value,
    /// A routine's days, a tracker's entries.
    pub history: serde_json::Value,
    /// A page's text, one block per line.
    pub text: Option<String>,
}

/// The whole export (the JSON file).
#[derive(Debug, Clone, Serialize)]
pub struct Export {
    pub format: &'static str,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    pub profile: CurrentUser,
    pub items: Vec<ExportedItem>,
    pub work_sessions: Vec<sessions::SessionSummary>,
}

fn json(value: impl Serialize) -> Result<serde_json::Value, AppError> {
    serde_json::to_value(value).map_err(AppError::internal)
}

/// An item that vanished or became hidden while the export ran is left out.
fn skip_gone<T>(result: Result<T, AppError>) -> Result<Option<T>, AppError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(AppError::NotFound | AppError::Forbidden) => Ok(None),
        Err(err) => Err(err),
    }
}

impl Sources {
    async fn item(
        &self,
        user: Uuid,
        item: Item,
        today: NaiveDate,
    ) -> Result<Option<ExportedItem>, AppError> {
        let id = item.id;
        let Some(tags) = skip_gone(self.items.item_tags(user, id).await)? else {
            return Ok(None);
        };
        let Some(links) = skip_gone(self.items.links(user, id).await)? else {
            return Ok(None);
        };
        let mut details = serde_json::Value::Null;
        let mut history = serde_json::Value::Null;
        let mut text = None;
        match item.kind.as_str() {
            "task" => {
                if let Some(task) = skip_gone(self.tasks.task(user, id).await)? {
                    details = json(task)?;
                }
            }
            "routine" => {
                if let Some(routine) = skip_gone(self.routines.routine(user, id).await)? {
                    let from = today
                        .checked_sub_days(Days::new(ROUTINE_DAYS))
                        .unwrap_or(today)
                        .max(routine.start_date);
                    if from <= today {
                        let days = self
                            .routines
                            .days(
                                user,
                                id,
                                routines::RoutineDaysQuery {
                                    from: Some(from),
                                    to: Some(today),
                                },
                            )
                            .await;
                        if let Some(days) = skip_gone(days)? {
                            history = json(days)?;
                        }
                    }
                    details = json(routine)?;
                }
            }
            "goal" => {
                if let Some(goal) = skip_gone(self.goals.goal(user, id).await)? {
                    details = json(goal)?;
                }
            }
            "tracker" => {
                if let Some(tracker) = skip_gone(self.trackers.tracker(user, id).await)? {
                    let mut entries = Vec::new();
                    let mut page = PageRequest {
                        limit: kernel::pagination::MAX_LIMIT,
                        cursor: None,
                    };
                    loop {
                        let Some(batch) =
                            skip_gone(self.trackers.list(user, id, (None, None), &page).await)?
                        else {
                            break;
                        };
                        entries.extend(batch.items);
                        match batch.next_cursor {
                            Some(cursor) => page.cursor = Some(cursor),
                            None => break,
                        }
                    }
                    history = json(entries)?;
                    details = json(tracker)?;
                }
            }
            "page" => {
                text = skip_gone(self.notebooks.page_text(user, id).await)?;
            }
            _ => {}
        }
        Ok(Some(ExportedItem {
            item,
            tags,
            links,
            details,
            history,
            text,
        }))
    }

    /// Everything `user` can see, as of `now`.
    ///
    /// # Errors
    /// [`AppError::Internal`] (or the user is gone: [`AppError::Unauthenticated`]).
    pub async fn collect(&self, user: Uuid, now: DateTime<Utc>) -> Result<Export, AppError> {
        let profile = self
            .identity
            .current_user(identity::UserId::from_uuid(user))
            .await?;
        let today = now.date_naive();
        let mut all = Vec::new();
        for state in [ListState::Active, ListState::Archived, ListState::Trash] {
            all.extend(self.items.list(user, state).await?);
        }
        all.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        all.dedup_by_key(|item| item.id);
        let mut exported = Vec::new();
        for item in all {
            if let Some(item) = self.item(user, item, today).await? {
                exported.push(item);
            }
        }
        let mut work_sessions = Vec::new();
        let mut before = None;
        loop {
            let batch = self.sessions.history(user, before).await?;
            let Some(last) = batch.last() else { break };
            before = Some(last.created_at);
            work_sessions.extend(batch);
        }
        Ok(Export {
            format: "routune-export",
            version: 1,
            exported_at: now,
            profile,
            items: exported,
            work_sessions,
        })
    }
}

/// Turkish section titles per kind, in the order the Markdown lists them.
const SECTIONS: [(&str, &str); 6] = [
    ("task", "Görevler"),
    ("routine", "Rutinler"),
    ("goal", "Hedefler"),
    ("tracker", "Takipler"),
    ("notebook", "Defterler"),
    ("page", "Sayfalar"),
];

fn state_of(item: &Item) -> &'static str {
    if item.deleted_at.is_some() {
        " (çöp kutusunda)"
    } else if item.archived_at.is_some() {
        " (arşivde)"
    } else {
        ""
    }
}

fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.get(key).filter(|v| !v.is_null())
}

fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The export as Markdown: the same data, for reading (HS3).
#[must_use]
pub fn markdown(export: &Export) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Routune verilerin\n");
    let _ = writeln!(
        out,
        "Dışa aktarma: {}\n",
        export.exported_at.format("%Y-%m-%d %H:%M UTC")
    );
    let _ = writeln!(out, "## Profil\n");
    let _ = writeln!(out, "- Ad: {}", export.profile.display_name);
    let _ = writeln!(out, "- E-posta: {}", export.profile.email);
    let _ = writeln!(out, "- Saat dilimi: {}\n", export.profile.timezone);
    let known: Vec<&str> = SECTIONS.iter().map(|(kind, _)| *kind).collect();
    let mut sections: Vec<(String, String)> = SECTIONS
        .iter()
        .map(|(kind, title)| ((*kind).to_owned(), (*title).to_owned()))
        .collect();
    let mut others: Vec<String> = export
        .items
        .iter()
        .map(|e| e.item.kind.clone())
        .filter(|kind| !known.contains(&kind.as_str()))
        .collect();
    others.sort();
    others.dedup();
    sections.extend(others.into_iter().map(|kind| (kind.clone(), kind)));
    for (kind, title) in sections {
        let items: Vec<&ExportedItem> = export
            .items
            .iter()
            .filter(|e| e.item.kind == kind)
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(out, "## {title}\n");
        for exported in items {
            item_markdown(&mut out, exported);
        }
    }
    if !export.work_sessions.is_empty() {
        let _ = writeln!(out, "## Çalışma oturumları\n");
        for session in &export.work_sessions {
            let _ = writeln!(
                out,
                "- {}: {} dk çalışma, {} dk mola",
                session.created_at.format("%Y-%m-%d %H:%M"),
                session.work_ms / 60_000,
                session.break_ms / 60_000
            );
        }
        out.push('\n');
    }
    out
}

fn item_markdown(out: &mut String, exported: &ExportedItem) {
    let item = &exported.item;
    let _ = writeln!(out, "### {}{}\n", item.title, state_of(item));
    if let Some(description) = item.description.as_deref().filter(|d| !d.trim().is_empty()) {
        let _ = writeln!(out, "{description}\n");
    }
    let details = &exported.details;
    for (key, label) in [
        ("start_date", "Başlangıç"),
        ("due_date", "Bitiş"),
        ("end_date", "Bitiş"),
        ("unit", "Birim"),
        ("target_value", "Hedef"),
    ] {
        if let Some(value) = field(details, key) {
            let _ = writeln!(out, "- {label}: {}", text_of(value));
        }
    }
    if !exported.tags.is_empty() {
        let names: Vec<&str> = exported.tags.iter().map(|t| t.name.as_str()).collect();
        let _ = writeln!(out, "- Etiketler: {}", names.join(", "));
    }
    if !exported.links.is_empty() {
        let titles: Vec<&str> = exported
            .links
            .iter()
            .map(|l| l.item.title.as_str())
            .collect();
        let _ = writeln!(out, "- Bağlantılar: {}", titles.join(", "));
    }
    if let Some(subtasks) = field(details, "subtasks").and_then(|s| s.as_array()) {
        for subtask in subtasks {
            let done = field(subtask, "done_at").is_some();
            let title = field(subtask, "title").map(text_of).unwrap_or_default();
            let _ = writeln!(out, "- [{}] {title}", if done { "x" } else { " " });
        }
    }
    if let Some(history) = exported.history.as_array() {
        if item.kind == "routine" {
            let done: Vec<String> = history
                .iter()
                .filter(|day| field(day, "complete").and_then(|c| c.as_bool()) == Some(true))
                .filter_map(|day| field(day, "date").map(text_of))
                .collect();
            let _ = writeln!(
                out,
                "- Tamamlanan günler ({}): {}",
                done.len(),
                done.join(", ")
            );
        } else {
            for entry in history {
                let date = field(entry, "local_date").map(text_of).unwrap_or_default();
                let value = field(entry, "value").map(text_of).unwrap_or_default();
                match field(entry, "note").map(text_of) {
                    Some(note) => {
                        let _ = writeln!(out, "- {date}: {value} ({note})");
                    }
                    None => {
                        let _ = writeln!(out, "- {date}: {value}");
                    }
                }
            }
        }
    }
    if let Some(text) = exported.text.as_deref().filter(|t| !t.is_empty()) {
        let _ = writeln!(out, "\n{text}");
    }
    out.push('\n');
}
