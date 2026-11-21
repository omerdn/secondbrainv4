//! Identity tables (T0.3.1, ARCHITECTURE §5.1, §15, ADR-0016) against a real Postgres 17.
//! Requires Docker (or `TEST_DATABASE_URL`).

// Integration-test file: panics are the failure mechanism, including in helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Duration, Utc};
use db::PgPool;
use db::testing::TestDb;
use uuid::Uuid;

/// Inserts a user and returns its id.
async fn insert_user(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Test')")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    id
}

/// A 32-byte value standing in for a SHA-256 digest.
fn digest() -> Vec<u8> {
    [Uuid::now_v7().into_bytes(), Uuid::now_v7().into_bytes()].concat()
}

/// A UUIDv4 (RFC 9562 variant, version 4).
const V4: Uuid = Uuid::from_u128(0x6f9619ff_8b86_4011_b42d_00c04fc964ff);
/// A non-RFC 9562 variant (NCS): `uuid_extract_version` returns NULL for it.
const NON_RFC: Uuid = Uuid::from_u128(0x0000_0000_0000_7000_0000_0000_0000_0001);

async fn insert_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(token_hash)
    .bind(Utc::now() + Duration::days(30))
    .execute(pool)
    .await
    .map(|_| ())
}

/// Name of the constraint (or unique index) a failed statement violated.
fn violated(result: Result<(), sqlx::Error>) -> String {
    let err = result.expect_err("statement should violate a constraint");
    err.as_database_error()
        .and_then(|e| e.constraint())
        .unwrap_or_else(|| panic!("not a constraint violation: {err}"))
        .to_owned()
}

async fn exec(pool: &PgPool, sql: &'static str, bind: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(sql).bind(bind).execute(pool).await.map(|_| ())
}

#[tokio::test]
async fn migrations_apply_on_a_clean_database() {
    let db = TestDb::new_unmigrated().await;
    db::migrate(db.pool()).await.unwrap();

    for table in ["users", "auth_identities", "sessions"] {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(table)
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(exists, "{table} missing");
    }
}

#[tokio::test]
async fn ids_are_uuid_and_timestamps_are_timestamptz() {
    let db = TestDb::new().await;
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text, data_type::text
         FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name IN ('users', 'auth_identities', 'sessions')
           AND (column_name = 'id' OR column_name LIKE '%\\_id' OR column_name LIKE '%\\_at')",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();

    assert!(columns.len() >= 10, "unexpected column set: {columns:?}");
    for (table, column, ty) in columns {
        let expected = if column.ends_with("_at") {
            "timestamp with time zone"
        } else {
            "uuid"
        };
        assert_eq!(ty, expected, "{table}.{column}");
    }
}

#[tokio::test]
async fn non_v7_ids_are_rejected() {
    let db = TestDb::new().await;
    let user = insert_user(db.pool(), "b@x.test").await;
    for bad in [V4, NON_RFC, Uuid::nil()] {
        let result =
            sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, 'a@x.test', 'A')")
                .bind(bad)
                .execute(db.pool())
                .await
                .map(|_| ());
        assert_eq!(violated(result), "users_id_check", "{bad}");

        let result = sqlx::query("INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, now() + interval '1 day')")
            .bind(bad)
            .bind(user)
            .bind(digest())
            .execute(db.pool())
            .await
            .map(|_| ());
        assert_eq!(violated(result), "sessions_id_check", "{bad}");
    }
}

#[tokio::test]
async fn email_is_unique_case_insensitively() {
    let db = TestDb::new().await;
    insert_user(db.pool(), "Ada@Example.test").await;

    let result = sqlx::query(
        "INSERT INTO users (id, email, display_name) VALUES ($1, 'ada@example.TEST', 'Ada')",
    )
    .bind(Uuid::now_v7())
    .execute(db.pool())
    .await
    .map(|_| ());
    assert_eq!(violated(result), "users_email_lower_key");
}

#[tokio::test]
async fn token_hash_is_unique_and_a_sha256_digest() {
    let db = TestDb::new().await;
    let user = insert_user(db.pool(), "s@x.test").await;
    let hash = digest();

    insert_session(db.pool(), user, &hash).await.unwrap();
    assert_eq!(
        violated(insert_session(db.pool(), user, &hash).await),
        "sessions_token_hash_key"
    );

    // A plain-text token (or anything that is not a 32-byte digest) cannot be stored.
    let plain = b"not-a-digest-just-a-raw-token-value-0123456789";
    assert_eq!(
        violated(insert_session(db.pool(), user, plain).await),
        "sessions_token_hash_check"
    );

    let column_type: String = sqlx::query_scalar(
        "SELECT data_type::text FROM information_schema.columns
         WHERE table_schema = current_schema() AND table_name = 'sessions' AND column_name = 'token_hash'",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(column_type, "bytea");
}

#[tokio::test]
async fn session_must_expire_after_creation() {
    let db = TestDb::new().await;
    let user = insert_user(db.pool(), "e@x.test").await;
    let result = sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, now() - interval '1 second')",
    )
    .bind(Uuid::now_v7())
    .bind(user)
    .bind(digest())
    .execute(db.pool())
    .await
    .map(|_| ());
    assert_eq!(violated(result), "sessions_expiry_check");
}

#[tokio::test]
async fn password_hash_is_required_only_for_password_identities() {
    let db = TestDb::new().await;
    let user = insert_user(db.pool(), "p@x.test").await;

    exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid, password_hash)
         VALUES ($1, 'password', $1::text, '$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA')",
        user,
    )
    .await
    .unwrap();
    exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'google', 'g-123')",
        user,
    )
    .await
    .unwrap();

    let other = insert_user(db.pool(), "q@x.test").await;
    let missing_hash = exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'password', $1::text)",
        other,
    )
    .await;
    assert_eq!(
        violated(missing_hash),
        "auth_identities_password_hash_check"
    );

    let oauth_with_hash = exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid, password_hash)
         VALUES ($1, 'apple', 'a-1', 'x')",
        other,
    )
    .await;
    assert_eq!(
        violated(oauth_with_hash),
        "auth_identities_password_hash_check"
    );

    let unknown = exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'github', 'gh-1')",
        other,
    )
    .await;
    assert_eq!(violated(unknown), "auth_identities_provider_check");
}

#[tokio::test]
async fn provider_accounts_are_unique() {
    let db = TestDb::new().await;
    let a = insert_user(db.pool(), "a@x.test").await;
    let b = insert_user(db.pool(), "b@x.test").await;
    let google = "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'google', 'g-1')";

    exec(db.pool(), google, a).await.unwrap();
    // The same Google account cannot belong to two users.
    assert_eq!(
        violated(exec(db.pool(), google, b).await),
        "auth_identities_pkey"
    );
    // A user has at most one identity per provider.
    let second = exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'google', 'g-2')",
        a,
    )
    .await;
    assert_eq!(violated(second), "auth_identities_user_id_provider_key");
}

#[tokio::test]
async fn deleting_a_user_removes_identities_and_sessions() {
    let db = TestDb::new().await;
    let user = insert_user(db.pool(), "d@x.test").await;
    exec(
        db.pool(),
        "INSERT INTO auth_identities (user_id, provider, provider_uid) VALUES ($1, 'apple', 'a-9')",
        user,
    )
    .await
    .unwrap();
    insert_session(db.pool(), user, &digest()).await.unwrap();

    exec(db.pool(), "DELETE FROM users WHERE id = $1", user)
        .await
        .unwrap();

    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM auth_identities) + (SELECT count(*) FROM sessions)",
    )
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(left, 0);
}
