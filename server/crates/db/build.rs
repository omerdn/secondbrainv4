//! `sqlx::migrate!` embeds the migrations at compile time but does not tell Cargo to watch the
//! directory, so a newly added migration would be missing from `db::MIGRATOR` until `db` is
//! rebuilt for another reason. This makes any change under `migrations/` trigger a rebuild.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
