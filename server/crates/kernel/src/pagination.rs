//! Cursor (keyset) pagination.
//!
//! Clients send `?limit=&cursor=` ([`PageParams`]) and receive a [`Page`] whose `next_cursor` is
//! an opaque string. Internally the cursor encodes the sort key of the last returned row, so a
//! query continues with `WHERE (sort_key, id) > (cursor_key)` instead of `OFFSET`, which stays
//! fast and stable while rows are inserted.
//!
//! Queries fetch [`PageRequest::fetch_limit`] rows (one extra) and hand them to
//! [`Page::from_overfetch`], which trims the extra row and builds the next cursor.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::validation::{FieldError, FieldErrorCode, ValidationErrors};

/// Page size when the client does not specify one.
pub const DEFAULT_LIMIT: u32 = 50;
/// Largest page size a client may request; larger values are clamped.
pub const MAX_LIMIT: u32 = 200;

/// Raw query-string parameters (`?limit=20&cursor=...`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageParams {
    /// Requested page size.
    pub limit: Option<u32>,
    /// Cursor from a previous page's `next_cursor`.
    pub cursor: Option<String>,
}

/// Validated pagination request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// Page size, `1..=MAX_LIMIT`.
    pub limit: u32,
    /// Where to continue from; `None` for the first page.
    pub cursor: Option<Cursor>,
}

impl PageRequest {
    /// Validates raw parameters. `limit = 0` and malformed cursors are rejected; limits above
    /// [`MAX_LIMIT`] are clamped.
    pub fn from_params(params: PageParams) -> Result<Self, AppError> {
        let limit = match params.limit {
            None => DEFAULT_LIMIT,
            Some(0) => {
                return Err(ValidationErrors::from(FieldError::new(
                    "limit",
                    FieldErrorCode::OutOfRange,
                    format!("must be between 1 and {MAX_LIMIT}"),
                ))
                .into());
            }
            Some(n) => n.min(MAX_LIMIT),
        };
        let cursor = params
            .cursor
            .filter(|c| !c.is_empty())
            .map(Cursor::parse)
            .transpose()?;
        Ok(Self { limit, cursor })
    }

    /// Number of rows to fetch: one more than `limit`, to detect whether another page exists.
    pub fn fetch_limit(&self) -> i64 {
        i64::from(self.limit) + 1
    }
}

/// Opaque continuation token. Clients must treat it as a black box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    /// Encodes a sort key (e.g. `(created_at, id)`) as a cursor.
    pub fn encode<K: Serialize>(key: &K) -> Self {
        // Serialising plain data (tuples, ids, timestamps) to JSON cannot fail; fall back to an
        // empty object rather than panicking.
        let json = serde_json::to_vec(key).unwrap_or_else(|_| b"{}".to_vec());
        Self(URL_SAFE_NO_PAD.encode(json))
    }

    /// Decodes the sort key. Fails with a `cursor` validation error if the cursor was tampered
    /// with or belongs to a different listing.
    pub fn decode<K: DeserializeOwned>(&self) -> Result<K, AppError> {
        URL_SAFE_NO_PAD
            .decode(&self.0)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(invalid_cursor)
    }

    /// Accepts a client-supplied cursor string after a cheap well-formedness check.
    pub fn parse(raw: String) -> Result<Self, AppError> {
        if URL_SAFE_NO_PAD.decode(&raw).is_ok() {
            Ok(Self(raw))
        } else {
            Err(invalid_cursor())
        }
    }

    /// The encoded string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn invalid_cursor() -> AppError {
    ValidationErrors::from(FieldError::new(
        "cursor",
        FieldErrorCode::Invalid,
        "invalid cursor",
    ))
    .into()
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Page<T> {
    /// Items on this page, at most `limit`.
    pub items: Vec<T>,
    /// Cursor for the next page; `None` on the last page.
    pub next_cursor: Option<Cursor>,
}

impl<T> Page<T> {
    /// Builds a page from up to `limit + 1` rows. If the extra row is present it is dropped and
    /// `next_cursor` encodes `key` of the last kept row.
    pub fn from_overfetch<K: Serialize>(
        mut rows: Vec<T>,
        limit: u32,
        key: impl Fn(&T) -> K,
    ) -> Self {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let next_cursor = if rows.len() > limit {
            rows.truncate(limit);
            rows.last().map(|last| Cursor::encode(&key(last)))
        } else {
            None
        };
        Self {
            items: rows,
            next_cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn params(limit: Option<u32>, cursor: Option<&str>) -> PageParams {
        PageParams {
            limit,
            cursor: cursor.map(str::to_owned),
        }
    }

    fn is_field_error(err: &AppError, field: &str) -> bool {
        matches!(err, AppError::Validation(e) if e.errors().iter().any(|f| f.field == field))
    }

    #[test]
    fn defaults_and_clamps_limit() {
        assert_eq!(
            PageRequest::from_params(params(None, None)).unwrap().limit,
            DEFAULT_LIMIT
        );
        assert_eq!(
            PageRequest::from_params(params(Some(10_000), None))
                .unwrap()
                .limit,
            MAX_LIMIT
        );
        assert_eq!(
            PageRequest::from_params(params(Some(1), None))
                .unwrap()
                .fetch_limit(),
            2
        );
    }

    #[test]
    fn rejects_zero_limit() {
        let err = PageRequest::from_params(params(Some(0), None)).unwrap_err();
        assert!(is_field_error(&err, "limit"));
    }

    #[test]
    fn rejects_malformed_cursor() {
        let err = PageRequest::from_params(params(None, Some("not base64!"))).unwrap_err();
        assert!(is_field_error(&err, "cursor"));
    }

    #[test]
    fn empty_cursor_means_first_page() {
        let req = PageRequest::from_params(params(None, Some(""))).unwrap();
        assert!(req.cursor.is_none());
    }

    #[test]
    fn cursor_roundtrips_sort_keys() {
        let key = ("2026-11-21T12:00:00Z".to_owned(), 42_u64);
        let cursor = Cursor::encode(&key);
        let parsed = Cursor::parse(cursor.as_str().to_owned()).unwrap();
        assert_eq!(parsed.decode::<(String, u64)>().unwrap(), key);
    }

    #[test]
    fn decoding_with_the_wrong_shape_is_a_cursor_error() {
        let cursor = Cursor::encode(&"just a string");
        let err = cursor.decode::<(String, u64)>().unwrap_err();
        assert!(is_field_error(&err, "cursor"));
    }

    #[test]
    fn page_serializes_with_next_cursor() {
        let page = Page::from_overfetch(vec![1, 2, 3], 2, |n| *n);
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["items"], serde_json::json!([1, 2]));
        assert!(json["next_cursor"].is_string());

        let last = Page::from_overfetch(vec![1, 2], 2, |n| *n);
        assert_eq!(
            serde_json::to_value(&last).unwrap()["next_cursor"],
            serde_json::Value::Null
        );
    }

    /// Simulates a keyset query: rows with key > cursor, ascending, `fetch_limit` of them.
    fn query(sorted: &[u32], req: &PageRequest) -> Vec<u32> {
        let after: Option<u32> = req.cursor.as_ref().map(|c| c.decode().unwrap());
        sorted
            .iter()
            .copied()
            .filter(|n| after.is_none_or(|a| *n > a))
            .take(usize::try_from(req.fetch_limit()).unwrap())
            .collect()
    }

    proptest! {
        #[test]
        fn overfetch_trims_and_flags_more(n in 0usize..50, limit in 1u32..20) {
            let rows: Vec<usize> = (0..n).collect();
            let page = Page::from_overfetch(rows, limit, |r| *r);
            let limit = usize::try_from(limit).unwrap();
            prop_assert_eq!(page.items.len(), n.min(limit));
            prop_assert_eq!(page.next_cursor.is_some(), n > limit);
        }

        /// Walking all pages returns every row exactly once, in order.
        #[test]
        fn paging_visits_every_row_once(
            mut rows in proptest::collection::btree_set(any::<u32>(), 0..120)
                .prop_map(|s| s.into_iter().collect::<Vec<_>>()),
            limit in 1u32..25,
        ) {
            rows.sort_unstable();
            let mut seen = Vec::new();
            let mut cursor: Option<String> = None;
            for _ in 0..=rows.len() {
                let req = PageRequest::from_params(PageParams { limit: Some(limit), cursor: cursor.clone() }).unwrap();
                let page = Page::from_overfetch(query(&rows, &req), req.limit, |n| *n);
                seen.extend(page.items);
                match page.next_cursor {
                    Some(next) => cursor = Some(next.as_str().to_owned()),
                    None => break,
                }
            }
            prop_assert_eq!(seen, rows);
        }
    }
}
