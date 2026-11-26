//! Item search over this crate's tables (PRD AR1, E2, T2.7.5): titles and descriptions through
//! the Turkish search document, titles also by trigram (typos, parts of words), and the caller's
//! own tags (E2: nobody else's tag names are searched). Archived items are found; trashed ones
//! are not. Only items the caller can view (AR2).

use access::Perms;
use kernel::AppError;
use kernel::search::{MatchField, SearchFuture, SearchSource, SourceHit};
use uuid::Uuid;

use crate::service::ItemsService;

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

/// Ranks: a title hit beats a tag, a tag beats the description.
const TITLE_BOOST: f32 = 3.0;
const TAG_RANK: f32 = 0.5;

impl ItemsService {
    async fn search_items(
        &self,
        actor: Uuid,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SourceHit>, AppError> {
        let visible = self
            .access
            .permissions()
            .list_accessible(&self.pool, actor, Perms::VIEW)
            .await
            .map_err(internal)?;
        if visible.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, f32, bool)> = sqlx::query_as(
            "WITH q AS (
                 SELECT search_query($2) AS tsq,
                        search_unaccent(lower($2)) AS raw
             )
             SELECT i.id,
                    GREATEST(
                        ts_rank(search_document(i.title || ' ' || coalesce(i.description, '')), q.tsq),
                        similarity(search_unaccent(lower(i.title)), q.raw)
                    )::real AS rank,
                    (search_document(i.title) @@ q.tsq
                     OR search_unaccent(lower(i.title)) % q.raw
                     OR strpos(search_unaccent(lower(i.title)), q.raw) > 0) AS in_title
             FROM items i, q
             WHERE i.id = ANY($1) AND i.deleted_at IS NULL
               AND (search_document(i.title || ' ' || coalesce(i.description, '')) @@ q.tsq
                    OR search_unaccent(lower(i.title)) % q.raw
                    OR search_unaccent(lower(i.title)) LIKE '%' || q.raw || '%')
             ORDER BY rank DESC, i.id
             LIMIT $3",
        )
        .bind(&visible)
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        let mut hits: Vec<SourceHit> = rows
            .into_iter()
            .map(|(item_id, rank, in_title)| SourceHit {
                item_id,
                rank: if in_title {
                    (rank + 0.1) * TITLE_BOOST
                } else {
                    rank
                },
                field: if in_title {
                    MatchField::Title
                } else {
                    MatchField::Description
                },
                snippet: None,
            })
            .collect();

        // The caller's own tags only (E2); a team's tags are not searched yet.
        let tagged: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT DISTINCT ON (it.item_id) it.item_id, t.name
             FROM tags t JOIN item_tags it ON it.tag_id = t.id
             JOIN items i ON i.id = it.item_id
             WHERE t.owner_user_id = $1 AND it.item_id = ANY($2) AND i.deleted_at IS NULL
               AND (search_unaccent(lower(t.name)) LIKE '%' || search_unaccent(lower($3)) || '%'
                    OR search_unaccent(lower(t.name)) % search_unaccent(lower($3)))
             ORDER BY it.item_id, t.name
             LIMIT $4",
        )
        .bind(actor)
        .bind(&visible)
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        hits.extend(tagged.into_iter().map(|(item_id, name)| SourceHit {
            item_id,
            rank: TAG_RANK,
            field: MatchField::Tag,
            snippet: Some(name),
        }));
        Ok(hits)
    }
}

impl SearchSource for ItemsService {
    fn search<'a>(&'a self, actor: Uuid, query: &'a str, limit: i64) -> SearchFuture<'a> {
        Box::pin(self.search_items(actor, query, limit))
    }
}
