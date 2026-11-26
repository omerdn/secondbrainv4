//! `NotebooksService`: notebooks and pages (PRD Y1, Y2, Y5, T2.5.1). Authorization goes through
//! `access::PermissionService`; items come from `ItemsService` (ADR-0007). A page's access comes
//! from its notebook through the `in_notebook` relation, so a page is visible to exactly those who
//! can see the notebook, plus any links later (V2).

use access::{AccessEngine, PermissionService, Perms, Relation, RelationTuple};
use items::{CreateItemRequest, ItemsAccessContext, ItemsService, Owner};
use kernel::{AppError, SharedClock};
use sqlx::PgPool;
use uuid::Uuid;

use base64::Engine;

use crate::collab::Collab;
use crate::model::{
    CreateNotebookRequest, CreatePageRequest, Notebook, NotebookSummary, Page, PageVersion,
    PageVersionContent, preview,
};
use crate::repo::{self, NotebookRow, PageRow, VersionRow};
use crate::snapshots;

/// Versions one history request returns.
const VERSIONS_PAGE: i64 = 50;

fn listed(row: VersionRow) -> PageVersion {
    PageVersion {
        version: row.version,
        created_at: row.created_at,
        preview: preview(&row.plain_text),
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Shared handle; cheap to clone.
#[derive(Clone)]
pub struct NotebooksService {
    pool: PgPool,
    clock: SharedClock,
    access: AccessEngine<ItemsAccessContext>,
    permissions: PermissionService<ItemsAccessContext>,
    items: ItemsService,
}

impl NotebooksService {
    #[must_use]
    pub fn new(
        pool: PgPool,
        clock: SharedClock,
        access: AccessEngine<ItemsAccessContext>,
        items: ItemsService,
    ) -> Self {
        let permissions = access.permissions();
        Self {
            pool,
            clock,
            access,
            permissions,
            items,
        }
    }

    /// The caller's default notebook, created the first time it is asked for (T2.5.1). Concurrent
    /// first calls create exactly one.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn ensure_default(&self, actor: Uuid, title: &str) -> Result<Uuid, AppError> {
        if let Some(id) = repo::default_of(&self.pool, actor)
            .await
            .map_err(internal)?
        {
            return Ok(id);
        }
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::lock_user(&mut tx, actor).await.map_err(internal)?;
        if let Some(id) = repo::default_of(&mut *tx, actor).await.map_err(internal)? {
            return Ok(id);
        }
        let new = CreateItemRequest {
            kind: "notebook".into(),
            title: title.into(),
            description: None,
        }
        .validate()?;
        let item = self.items.create_in(&mut tx, actor, new).await?;
        let row = NotebookRow {
            item_id: item.id,
            default_for: Some(actor),
        };
        repo::insert_notebook(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(item.id)
    }

    /// Creates another personal notebook (Y1).
    ///
    /// # Errors
    /// [`AppError::Validation`] or [`AppError::Internal`].
    pub async fn create(
        &self,
        actor: Uuid,
        request: CreateNotebookRequest,
    ) -> Result<Notebook, AppError> {
        let new = CreateItemRequest {
            kind: "notebook".into(),
            title: request.title,
            description: request.description,
        }
        .validate()?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self.items.create_in(&mut tx, actor, new).await?;
        let row = NotebookRow {
            item_id: item.id,
            default_for: None,
        };
        repo::insert_notebook(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.notebook(actor, item.id).await
    }

    /// Notebooks the caller can see and that are not in the trash; the default one first.
    ///
    /// # Errors
    /// [`AppError::Internal`].
    pub async fn notebooks(&self, actor: Uuid) -> Result<Vec<NotebookSummary>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        let rows = repo::notebooks_among(&self.pool, &visible)
            .await
            .map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let items = self.items.visible_items(actor, &ids).await?;
        Ok(rows
            .into_iter()
            .zip(ids)
            .filter_map(|(row, id)| {
                let item = items.iter().find(|item| item.id == id)?.clone();
                (item.deleted_at.is_none()).then_some(NotebookSummary {
                    item,
                    is_default: row.default_for.is_some(),
                })
            })
            .collect())
    }

    /// A notebook with the pages the caller can see (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not visible or not a notebook, or [`AppError::Internal`].
    pub async fn notebook(&self, actor: Uuid, id: Uuid) -> Result<Notebook, AppError> {
        let item = self.items.get(actor, id).await?;
        let row = repo::get_notebook(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let rows = repo::pages_of(&self.pool, id).await.map_err(internal)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.item_id).collect();
        let items = self.items.visible_items(actor, &ids).await?;
        let pages = rows
            .into_iter()
            .filter_map(|row| {
                let item = items.iter().find(|item| item.id == row.item_id)?.clone();
                item.deleted_at.is_none().then_some(Page {
                    item,
                    notebook_id: row.notebook_id,
                    process_of_item_id: row.process_of_item_id,
                })
            })
            .collect();
        Ok(Notebook {
            item,
            is_default: row.default_for.is_some(),
            pages,
        })
    }

    /// Adds a page to a notebook (EDIT on the notebook). The page belongs to the notebook's owner,
    /// whoever creates it (Y5), and its access follows the notebook.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`], [`AppError::Validation`],
    /// [`AppError::Conflict`] for a notebook in the trash, or [`AppError::Internal`].
    pub async fn create_page(
        &self,
        actor: Uuid,
        notebook: Uuid,
        request: CreatePageRequest,
    ) -> Result<Page, AppError> {
        self.permissions
            .require(&self.pool, actor, notebook, Perms::EDIT)
            .await?;
        let notebook_item = self.items.get(actor, notebook).await?;
        repo::get_notebook(&self.pool, notebook)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        if notebook_item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        let Owner::User { user_id: owner } = notebook_item.owner else {
            return Err(AppError::Conflict(
                "team notebooks are not available yet".into(),
            ));
        };
        let new = CreateItemRequest {
            kind: "page".into(),
            title: request.title,
            description: None,
        }
        .validate()?;
        let mut tx = self.pool.begin().await.map_err(internal)?;
        let item = self
            .items
            .create_owned_in(&mut tx, actor, owner, new)
            .await?;
        let row = PageRow {
            item_id: item.id,
            notebook_id: notebook,
            process_of_item_id: None,
        };
        repo::insert_page(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        self.access
            .grant(
                &mut tx,
                RelationTuple::new(item.id, Relation::InNotebook, notebook),
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        self.page(actor, item.id).await
    }

    /// Creates the process page of `item` in `notebook` of `owner` (S1, S2) unless it has one,
    /// and returns it. No permission check: callers check access to the item first. Concurrent
    /// calls create exactly one.
    ///
    /// # Errors
    /// [`AppError::Validation`] for a bad title, or [`AppError::Internal`].
    pub(crate) async fn create_process_page(
        &self,
        (actor, owner): (Uuid, Uuid),
        notebook: Uuid,
        item: Uuid,
        title: String,
    ) -> Result<Uuid, AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        repo::lock_process_page(&mut tx, item)
            .await
            .map_err(internal)?;
        if let Some(page) = repo::process_page_of(&mut *tx, item)
            .await
            .map_err(internal)?
        {
            return Ok(page);
        }
        let new = CreateItemRequest {
            kind: "page".into(),
            title,
            description: None,
        }
        .validate()?;
        let page = self
            .items
            .create_owned_in(&mut tx, actor, owner, new)
            .await?;
        let row = PageRow {
            item_id: page.id,
            notebook_id: notebook,
            process_of_item_id: Some(item),
        };
        repo::insert_page(&mut tx, row, self.clock.now())
            .await
            .map_err(internal)?;
        self.access
            .grant(
                &mut tx,
                RelationTuple::new(page.id, Relation::InNotebook, notebook),
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;
        Ok(page.id)
    }

    /// A page (VIEW).
    ///
    /// # Errors
    /// [`AppError::NotFound`] or [`AppError::Internal`].
    pub async fn page(&self, actor: Uuid, id: Uuid) -> Result<Page, AppError> {
        let item = self.items.get(actor, id).await?;
        let row = repo::get_page(&self.pool, id)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(Page {
            item,
            notebook_id: row.notebook_id,
            process_of_item_id: row.process_of_item_id,
        })
    }

    /// A page's version history, newest first, older than `before` if given (VIEW, Y3).
    ///
    /// # Errors
    /// [`AppError::NotFound`] or [`AppError::Internal`].
    pub async fn versions(
        &self,
        actor: Uuid,
        page: Uuid,
        before: Option<i32>,
    ) -> Result<Vec<PageVersion>, AppError> {
        self.page(actor, page).await?;
        let rows = repo::versions(&self.pool, page, before, VERSIONS_PAGE)
            .await
            .map_err(internal)?;
        Ok(rows.into_iter().map(listed).collect())
    }

    /// One version's content (VIEW, Y3).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the page or the version is not there, or [`AppError::Internal`].
    pub async fn version(
        &self,
        actor: Uuid,
        page: Uuid,
        version: i32,
    ) -> Result<PageVersionContent, AppError> {
        self.page(actor, page).await?;
        let (row, state) = repo::snapshot(&self.pool, page, version)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        Ok(PageVersionContent {
            version: row.version,
            created_at: row.created_at,
            plain_text: row.plain_text,
            state: base64::engine::general_purpose::STANDARD.encode(state),
        })
    }

    /// Brings back an old version as a new change (EDIT, Y3, ARCHITECTURE §9). The current
    /// content is kept as a version first and the result becomes the newest version, so nothing
    /// in the history is lost; open editors receive the change at once.
    ///
    /// # Errors
    /// [`AppError::NotFound`], [`AppError::Forbidden`] without EDIT, [`AppError::Conflict`] for a
    /// page in the trash, or [`AppError::Internal`].
    pub async fn restore_version(
        &self,
        actor: Uuid,
        page: Uuid,
        version: i32,
        collab: &Collab,
    ) -> Result<PageVersion, AppError> {
        let item = self.page(actor, page).await?.item;
        self.permissions
            .require(&self.pool, actor, page, Perms::EDIT)
            .await?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        let (_, state) = repo::snapshot(&self.pool, page, version)
            .await
            .map_err(internal)?
            .ok_or(AppError::NotFound)?;
        let target = snapshots::rebuild(Some(&state), &[]).map_err(internal)?;
        snapshots::compact(&self.pool, page, self.clock.now())
            .await
            .map_err(internal)?;
        collab.restore(page, &target, actor).await?;
        snapshots::compact(&self.pool, page, self.clock.now())
            .await
            .map_err(internal)?;
        let latest = repo::versions(&self.pool, page, None, 1)
            .await
            .map_err(internal)?
            .pop()
            .ok_or_else(|| AppError::Internal("restored page has no version".into()))?;
        Ok(listed(latest))
    }
}
