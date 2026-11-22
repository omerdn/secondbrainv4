//! Personal tags (PRD E1, E2, E4, T1.2.2). A tag is visible to its owner only, even on a shared
//! item, so every query is scoped to the owner. Access to the item itself goes through
//! `access::PermissionService`. Team tags arrive with F5.

use access::Perms;
use chrono::{DateTime, Utc};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, ValidationErrors};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use ts_rs::TS;
use uuid::Uuid;

use crate::model::{Item, ListState};
use crate::service::ItemsService;

pub const TAG_NAME_MAX_CHARS: usize = 50;

/// Most tags one list returns.
pub const TAG_LIST_LIMIT: i64 = 500;

const TAG_COLUMNS: &str = "id, name, color, created_at, updated_at";

/// A tag as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow, TS)]
#[ts(export)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`, lowercase.
    pub color: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `POST /tags` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateTagRequest {
    pub name: String,
    #[serde(default)]
    #[ts(optional)]
    pub color: Option<String>,
}

/// `PATCH /tags/{id}` body: only the fields present change. An empty `color` clears it.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateTagRequest {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub color: Option<String>,
}

/// A validated new tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTag {
    pub name: String,
    pub color: Option<String>,
}

/// Validated changes; `color: Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagChanges {
    pub name: Option<String>,
    pub color: Option<Option<String>>,
}

fn check_name(errors: &mut ValidationErrors, name: &str) {
    errors.check(non_blank("name", name));
    errors.check(max_chars("name", name, TAG_NAME_MAX_CHARS));
}

/// Lowercase `#rrggbb`, or `None` if `color` is not a hex color.
fn normalize_color(color: &str) -> Option<String> {
    let hex = color.trim().strip_prefix('#')?;
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", hex.to_ascii_lowercase()))
}

fn check_color(errors: &mut ValidationErrors, color: &str) -> Option<String> {
    let normalized = normalize_color(color);
    if normalized.is_none() {
        errors.push(FieldError::new(
            "color",
            FieldErrorCode::Invalid,
            "expected a #rrggbb color",
        ));
    }
    normalized
}

impl CreateTagRequest {
    /// # Errors
    /// [`AppError::Validation`] for a blank or long name or an invalid color.
    pub fn validate(self) -> Result<NewTag, AppError> {
        let mut errors = ValidationErrors::new();
        let name = self.name.trim().to_owned();
        check_name(&mut errors, &name);
        let color = self.color.and_then(|c| check_color(&mut errors, &c));
        errors.into_result()?;
        Ok(NewTag { name, color })
    }
}

impl UpdateTagRequest {
    /// # Errors
    /// [`AppError::Validation`] for a blank or long name, an invalid color, or no field.
    pub fn validate(self) -> Result<TagChanges, AppError> {
        let mut errors = ValidationErrors::new();
        let name = self.name.map(|n| n.trim().to_owned());
        if let Some(name) = &name {
            check_name(&mut errors, name);
        }
        let color = self.color.map(|c| {
            if c.trim().is_empty() {
                None
            } else {
                check_color(&mut errors, &c)
            }
        });
        if name.is_none() && color.is_none() {
            errors.push(FieldError::new(
                "name",
                FieldErrorCode::Required,
                "nothing to change",
            ));
        }
        errors.into_result()?;
        Ok(TagChanges { name, color })
    }
}

fn internal(err: impl std::error::Error + Send + Sync + 'static) -> AppError {
    AppError::Internal(Box::new(err))
}

fn name_conflict(err: sqlx::Error) -> AppError {
    if err
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        AppError::Conflict("a tag with this name already exists".into())
    } else {
        internal(err)
    }
}

impl ItemsService {
    /// Creates a personal tag owned by `actor`.
    ///
    /// # Errors
    /// [`AppError::Conflict`] if `actor` already has a tag with this name, or
    /// [`AppError::Internal`].
    pub async fn create_tag(&self, actor: Uuid, new: NewTag) -> Result<Tag, AppError> {
        let now = self.clock.now();
        let id = kernel::Id::<()>::new_at(now).as_uuid();
        sqlx::query_as::<_, Tag>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO tags (id, owner_user_id, name, color, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $5)
             RETURNING {TAG_COLUMNS}"
        )))
        .bind(id)
        .bind(actor)
        .bind(&new.name)
        .bind(&new.color)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(name_conflict)
    }

    /// The tags `actor` can see, by name (E4).
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn list_tags(&self, actor: Uuid) -> Result<Vec<Tag>, AppError> {
        sqlx::query_as::<_, Tag>(sqlx::AssertSqlSafe(format!(
            "SELECT {TAG_COLUMNS} FROM tags WHERE owner_user_id = $1
             ORDER BY lower(name), id LIMIT $2"
        )))
        .bind(actor)
        .bind(TAG_LIST_LIMIT)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)
    }

    /// Renames or recolors one of `actor`'s tags.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not theirs (E2), [`AppError::Conflict`] for a taken name,
    /// or [`AppError::Internal`].
    pub async fn update_tag(
        &self,
        actor: Uuid,
        id: Uuid,
        changes: TagChanges,
    ) -> Result<Tag, AppError> {
        let (set_color, color) = match changes.color {
            Some(c) => (true, c),
            None => (false, None),
        };
        sqlx::query_as::<_, Tag>(sqlx::AssertSqlSafe(format!(
            "UPDATE tags SET name = COALESCE($3, name),
                             color = CASE WHEN $4 THEN $5 ELSE color END,
                             updated_at = $6
             WHERE id = $1 AND owner_user_id = $2
             RETURNING {TAG_COLUMNS}"
        )))
        .bind(id)
        .bind(actor)
        .bind(&changes.name)
        .bind(set_color)
        .bind(&color)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(name_conflict)?
        .ok_or(AppError::NotFound)
    }

    /// Deletes one of `actor`'s tags and detaches it from every item.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not theirs (E2), or [`AppError::Internal`].
    pub async fn delete_tag(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        let deleted = sqlx::query("DELETE FROM tags WHERE id = $1 AND owner_user_id = $2")
            .bind(id)
            .bind(actor)
            .execute(&self.pool)
            .await
            .map_err(internal)?
            .rows_affected();
        if deleted == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// `actor`'s own tags on an item they can view; other members' tags never appear (E2).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the item is not visible (V9), or [`AppError::Internal`].
    pub async fn item_tags(&self, actor: Uuid, item_id: Uuid) -> Result<Vec<Tag>, AppError> {
        self.require(actor, item_id, Perms::VIEW).await?;
        sqlx::query_as::<_, Tag>(
            "SELECT t.id, t.name, t.color, t.created_at, t.updated_at
             FROM item_tags it JOIN tags t ON t.id = it.tag_id
             WHERE it.item_id = $1 AND t.owner_user_id = $2
             ORDER BY lower(t.name), t.id",
        )
        .bind(item_id)
        .bind(actor)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)
    }

    /// Attaches one of `actor`'s tags to an item they can view. A personal tag does not change
    /// the item for anyone else, so VIEW is enough. Attaching twice is a no-op.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a tag or item that is not visible, [`AppError::Conflict`] for
    /// an item in the trash, or [`AppError::Internal`].
    pub async fn attach_tag(
        &self,
        actor: Uuid,
        item_id: Uuid,
        tag_id: Uuid,
    ) -> Result<(), AppError> {
        self.require_own_tag(actor, tag_id).await?;
        self.require(actor, item_id, Perms::VIEW).await?;
        let item = self.load(item_id).await?.ok_or(AppError::NotFound)?;
        if item.deleted_at.is_some() {
            return Err(AppError::Conflict(
                "the item is in the trash; restore it first".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO item_tags (item_id, tag_id, tagged_by, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (item_id, tag_id) DO NOTHING",
        )
        .bind(item_id)
        .bind(tag_id)
        .bind(actor)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    /// Detaches one of `actor`'s tags from an item. Detaching an absent tag is a no-op.
    ///
    /// # Errors
    /// [`AppError::NotFound`] for a tag or item that is not visible, or [`AppError::Internal`].
    pub async fn detach_tag(
        &self,
        actor: Uuid,
        item_id: Uuid,
        tag_id: Uuid,
    ) -> Result<(), AppError> {
        self.require_own_tag(actor, tag_id).await?;
        self.require(actor, item_id, Perms::VIEW).await?;
        sqlx::query("DELETE FROM item_tags WHERE item_id = $1 AND tag_id = $2")
            .bind(item_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// Items carrying one of `actor`'s tags, limited to those `actor` can still access (E4).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if the tag is not theirs (E2), or [`AppError::Internal`].
    pub async fn items_by_tag(
        &self,
        actor: Uuid,
        tag_id: Uuid,
        state: ListState,
    ) -> Result<Vec<Item>, AppError> {
        self.require_own_tag(actor, tag_id).await?;
        self.list_where(actor, state, Some(tag_id)).await
    }

    async fn require_own_tag(&self, actor: Uuid, tag_id: Uuid) -> Result<(), AppError> {
        let own: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tags WHERE id = $1 AND owner_user_id = $2)",
        )
        .bind(tag_id)
        .bind(actor)
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if own { Ok(()) } else { Err(AppError::NotFound) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(normalize_color(" #A1b2C3 ").as_deref(), Some("#a1b2c3"));
        for bad in ["a1b2c3", "#a1b2c", "#a1b2c3d", "#g1b2c3", ""] {
            assert_eq!(normalize_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn create_validation() {
        let ok = CreateTagRequest {
            name: "  İş ".into(),
            color: Some("#FF0000".into()),
        }
        .validate()
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ok.name, "İş");
        assert_eq!(ok.color.as_deref(), Some("#ff0000"));
        for (name, color) in [
            ("  ".to_owned(), None),
            ("x".repeat(TAG_NAME_MAX_CHARS + 1), None),
            ("ok".to_owned(), Some("red".to_owned())),
        ] {
            assert!(CreateTagRequest { name, color }.validate().is_err());
        }
    }

    #[test]
    fn update_validation() {
        let cleared = UpdateTagRequest {
            name: None,
            color: Some(String::new()),
        }
        .validate()
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cleared.color, Some(None), "empty color clears it");
        assert!(UpdateTagRequest::default().validate().is_err());
        assert!(
            UpdateTagRequest {
                name: None,
                color: Some("red".into()),
            }
            .validate()
            .is_err()
        );
    }
}
