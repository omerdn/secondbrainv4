//! Page content search over this crate's tables (PRD AR1, T2.7.5): the latest snapshot's plain
//! text of every page the caller can view, through the Turkish search document. Text typed
//! since the last compaction is found after the next one.

use access::Perms;
use kernel::AppError;
use kernel::search::{MatchField, SearchFuture, SearchSource, SourceHit};
use uuid::Uuid;

use crate::service::NotebooksService;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

impl NotebooksService {
    async fn search_pages(
        &self,
        actor: Uuid,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SourceHit>, AppError> {
        let visible = self
            .permissions
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, f32, String)> = sqlx::query_as(
            "WITH latest AS (
                 SELECT DISTINCT ON (page_id) page_id, plain_text
                 FROM page_snapshots WHERE page_id = ANY($1)
                 ORDER BY page_id, version DESC
             ), q AS (SELECT search_query($2) AS tsq)
             SELECT l.page_id,
                    ts_rank(search_document(l.plain_text), q.tsq)::real,
                    ts_headline('pg_catalog.turkish', l.plain_text, q.tsq,
                                'StartSel=\"\",StopSel=\"\",MaxWords=18,MinWords=6,MaxFragments=1')
             FROM latest l, q
             WHERE search_document(l.plain_text) @@ q.tsq
             ORDER BY 2 DESC, l.page_id
             LIMIT $3",
        )
        .bind(&visible)
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|(item_id, rank, snippet)| SourceHit {
                item_id,
                rank,
                field: MatchField::Content,
                snippet: Some(snippet).filter(|s| !s.trim().is_empty()),
            })
            .collect())
    }
}

impl SearchSource for NotebooksService {
    fn search<'a>(&'a self, actor: Uuid, query: &'a str, limit: i64) -> SearchFuture<'a> {
        Box::pin(self.search_pages(actor, query, limit))
    }
}
