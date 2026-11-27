//! Saved views (PRD F2, T1.2.7): a named filter a person keeps and may pin to the sidebar. Views
//! are private to their owner. Running one goes through the filter engine, so it always reflects
//! what the owner can access at that moment.

use chrono::{DateTime, Utc};
use kernel::validation::{max_chars, non_blank};
use kernel::{AppError, FieldError, FieldErrorCode, Page, PageRequest, ValidationErrors};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::types::Json;
use ts_rs::TS;
use uuid::Uuid;

use crate::filters::ItemFilter;
use crate::model::Item;
use crate::service::ItemsService;

pub const VIEW_NAME_MAX_CHARS: usize = 60;
/// Most saved views one person can keep.
pub const VIEW_LIMIT: i64 = 100;
const POSITION_MAX: i32 = 10_000;

const VIEW_COLUMNS: &str = "id, name, filter, pinned, position, created_at, updated_at";

/// A saved view as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SavedView {
    pub id: Uuid,
    pub name: String,
    pub filter: ItemFilter,
    /// Shown in the sidebar (F2).
    pub pinned: bool,
    /// Order among the views; lower comes first.
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `POST /saved-views` body.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct CreateSavedViewRequest {
    pub name: String,
    pub filter: ItemFilter,
    #[serde(default)]
    #[ts(optional)]
    pub pinned: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub position: Option<i32>,
}

/// `PATCH /saved-views/{id}` body: only the fields present change.
#[derive(Debug, Clone, Default, Deserialize, TS)]
#[ts(export)]
pub struct UpdateSavedViewRequest {
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub filter: Option<ItemFilter>,
    #[serde(default)]
    #[ts(optional)]
    pub pinned: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub position: Option<i32>,
}

fn check_name(errors: &mut ValidationErrors, name: &str) {
    errors.check(non_blank("name", name));
    errors.check(max_chars("name", name, VIEW_NAME_MAX_CHARS));
}

fn check_position(errors: &mut ValidationErrors, position: Option<i32>) {
    if position.is_some_and(|p| !(0..=POSITION_MAX).contains(&p)) {
        errors.push(FieldError::new(
            "position",
            FieldErrorCode::OutOfRange,
            format!("must be between 0 and {POSITION_MAX}"),
        ));
    }
}

impl CreateSavedViewRequest {
    /// # Errors
    /// [`AppError::Validation`] for a bad name, position or filter.
    pub fn validate(mut self) -> Result<Self, AppError> {
        self.name = self.name.trim().to_owned();
        let mut errors = ValidationErrors::new();
        check_name(&mut errors, &self.name);
        check_position(&mut errors, self.position);
        errors.into_result()?;
        self.filter.validate()?;
        Ok(self)
    }
}

impl UpdateSavedViewRequest {
    /// # Errors
    /// [`AppError::Validation`] for a bad name, position or filter, or no field.
    pub fn validate(mut self) -> Result<Self, AppError> {
        self.name = self.name.map(|name| name.trim().to_owned());
        let mut errors = ValidationErrors::new();
        if let Some(name) = &self.name {
            check_name(&mut errors, name);
        }
        check_position(&mut errors, self.position);
        if self.name.is_none()
            && self.filter.is_none()
            && self.pinned.is_none()
            && self.position.is_none()
        {
            errors.push(FieldError::new(
                "name",
                FieldErrorCode::Required,
                "nothing to change",
            ));
        }
        errors.into_result()?;
        if let Some(filter) = &self.filter {
            filter.validate()?;
        }
        Ok(self)
    }
}

#[derive(FromRow)]
struct ViewRow {
    id: Uuid,
    name: String,
    filter: Json<ItemFilter>,
    pinned: bool,
    position: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ViewRow> for SavedView {
    fn from(row: ViewRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            filter: row.filter.0,
            pinned: row.pinned,
            position: row.position,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
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
        AppError::Conflict("a view with this name already exists".into())
    } else {
        internal(err)
    }
}

impl ItemsService {
    /// Saves a view for `actor`.
    ///
    /// # Errors
    /// [`AppError::Conflict`] for a taken name or at [`VIEW_LIMIT`], or [`AppError::Internal`].
    pub async fn create_view(
        &self,
        actor: Uuid,
        new: CreateSavedViewRequest,
    ) -> Result<SavedView, AppError> {
        new.filter.validate()?;
        self.check_people(actor, &new.filter).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM saved_views WHERE user_id = $1")
            .bind(actor)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
        if count >= VIEW_LIMIT {
            return Err(AppError::Conflict("too many saved views".into()));
        }
        let now = self.clock.now();
        let row = sqlx::query_as::<_, ViewRow>(sqlx::AssertSqlSafe(format!(
            "INSERT INTO saved_views (id, user_id, name, filter, pinned, position, created_at,
                                      updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
             RETURNING {VIEW_COLUMNS}"
        )))
        .bind(kernel::Id::<()>::new_at(now).as_uuid())
        .bind(actor)
        .bind(&new.name)
        .bind(Json(&new.filter))
        .bind(new.pinned.unwrap_or(false))
        .bind(new.position.unwrap_or(0))
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(name_conflict)?;
        Ok(row.into())
    }

    /// `actor`'s views: pinned first, then by position and name.
    ///
    /// # Errors
    /// [`AppError::Internal`] on database errors.
    pub async fn list_views(&self, actor: Uuid) -> Result<Vec<SavedView>, AppError> {
        let rows = sqlx::query_as::<_, ViewRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {VIEW_COLUMNS} FROM saved_views WHERE user_id = $1
             ORDER BY pinned DESC, position, lower(name), id"
        )))
        .bind(actor)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(SavedView::from).collect())
    }

    /// Changes one of `actor`'s views; pinning is a change of `pinned` (F2).
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not theirs, [`AppError::Conflict`] for a taken name, or
    /// [`AppError::Internal`].
    pub async fn update_view(
        &self,
        actor: Uuid,
        id: Uuid,
        changes: UpdateSavedViewRequest,
    ) -> Result<SavedView, AppError> {
        if let Some(filter) = &changes.filter {
            filter.validate()?;
            self.check_people(actor, filter).await?;
        }
        let row = sqlx::query_as::<_, ViewRow>(sqlx::AssertSqlSafe(format!(
            "UPDATE saved_views SET name = COALESCE($3, name),
                                    filter = COALESCE($4, filter),
                                    pinned = COALESCE($5, pinned),
                                    position = COALESCE($6, position),
                                    updated_at = $7
             WHERE id = $1 AND user_id = $2
             RETURNING {VIEW_COLUMNS}"
        )))
        .bind(id)
        .bind(actor)
        .bind(&changes.name)
        .bind(changes.filter.as_ref().map(Json))
        .bind(changes.pinned)
        .bind(changes.position)
        .bind(self.clock.now())
        .fetch_optional(&self.pool)
        .await
        .map_err(name_conflict)?;
        row.map(SavedView::from).ok_or(AppError::NotFound)
    }

    /// Deletes one of `actor`'s views.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not theirs, or [`AppError::Internal`].
    pub async fn delete_view(&self, actor: Uuid, id: Uuid) -> Result<(), AppError> {
        let deleted = sqlx::query("DELETE FROM saved_views WHERE id = $1 AND user_id = $2")
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

    /// Runs one of `actor`'s views.
    ///
    /// # Errors
    /// [`AppError::NotFound`] if it is not theirs, [`AppError::Validation`] for a bad cursor, or
    /// [`AppError::Internal`].
    pub async fn view_items(
        &self,
        actor: Uuid,
        id: Uuid,
        page: &PageRequest,
    ) -> Result<Page<Item>, AppError> {
        let filter: Option<Json<ItemFilter>> =
            sqlx::query_scalar("SELECT filter FROM saved_views WHERE id = $1 AND user_id = $2")
                .bind(id)
                .bind(actor)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let filter = filter.ok_or(AppError::NotFound)?;
        self.filter_items(actor, &filter.0, page).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(name: &str, position: Option<i32>) -> CreateSavedViewRequest {
        CreateSavedViewRequest {
            name: name.into(),
            filter: ItemFilter::default(),
            pinned: None,
            position,
        }
    }

    #[test]
    fn create_validation() {
        let ok = create("  Work ", Some(3))
            .validate()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ok.name, "Work");
        assert!(create("  ", None).validate().is_err());
        assert!(
            create(&"x".repeat(VIEW_NAME_MAX_CHARS + 1), None)
                .validate()
                .is_err()
        );
        assert!(create("ok", Some(-1)).validate().is_err());
        assert!(create("ok", Some(POSITION_MAX + 1)).validate().is_err());
    }

    #[test]
    fn update_needs_a_field() {
        assert!(UpdateSavedViewRequest::default().validate().is_err());
        let pin = UpdateSavedViewRequest {
            pinned: Some(true),
            ..UpdateSavedViewRequest::default()
        };
        assert!(pin.validate().is_ok());
    }
}
