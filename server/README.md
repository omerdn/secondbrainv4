# server/

Rust modüler monolit için Cargo workspace (ARCHITECTURE §3–§4, ADR-0007). Bu klasör pnpm workspace'inin parçası değildir.

## Yapı
- `crates/app` — binary (sunucu başlatma, router birleştirme)
- `crates/<modül>` — her domain modülü ayrı bir crate: `kernel`, `db`, `identity`, `access`, `items`, `tracking`, `recurrence`, `tasks`, `routines`, `goals`, `notebooks`, `sessions`, `social`, `teams`, `notifications`, `search`, `realtime`, `jobs`

Crate listesi ARCHITECTURE §3 ile `crates/app/tests/workspace_layout.rs` testiyle eşleştirilir. Modül eklemek veya kaldırmak bir mimari değişikliktir: dokümanı, ADR'yi ve bu testi birlikte güncelleyin.

## Kurallar
- Rust sürümü kökteki `rust-toolchain.toml` ile sabitlenmiştir (rustup otomatik kurar).
- Lint ayarları `Cargo.toml` → `[workspace.lints]` içindedir. Her crate `[lints] workspace = true` ile devralır (testle zorlanır). `unsafe_code` yasaktır.
- Crate'ler arası bağımlılıklar, ilgili görev implemente edilirken ARCHITECTURE §4.3'teki tek yönlü sıraya göre eklenir.

- Bağımlılık sürümleri `Cargo.toml` → `[workspace.dependencies]` içinde tek yerde tutulur; crate'ler `<ad>.workspace = true` ile kullanır.
- Entegrasyon testi dosyaları (`tests/*.rs`) en üstte `#![allow(clippy::unwrap_used, clippy::expect_used)]` taşır. Clippy'nin testlerdeki istisnası yardımcı fonksiyonları kapsamıyor.

## Veritabanı ve migration'lar
- Migration'lar `server/migrations/` altında, sqlx formatında: `<YYYYMMDDHHMMSS>_<açıklama>.sql`. Derleme sırasında binary'ye gömülür (`db::MIGRATOR`).
- Sunucu açılışta bekleyen migration'ları uygular (`DB_MIGRATE_ON_START=false` ile kapatılabilir). Birden fazla örnek aynı anda başlarsa sqlx advisory lock ile sıraya girer.
- **Merge edilmiş bir migration asla düzenlenmez.** Değişiklik için yeni bir migration eklenir.
- Her domain crate'i sadece kendi tablolarının migration'larını yazar (ADR-0007). Tablo adları modül önekiyle anlaşılır olmalı.
- Yeni dosya elle oluşturulabilir veya [sqlx-cli](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli) ile: `sqlx migrate add <açıklama>` (isteğe bağlı kurulum).

## Veritabanı testleri
`db::testing::TestDb` (feature `testing`) her test için ayrı bir şema açar, tüm migration'ları o şemada uygular ve o şemaya bağlı bir pool verir:

```rust
// Cargo.toml → [dev-dependencies] db = { path = "../db", features = ["testing"] }
let db = db::testing::TestDb::new().await;
sqlx::query("...").execute(db.pool()).await?;
```

- İlk kullanımda testcontainers ile **tek bir Postgres 17 konteyneri** (`secondbrainv4-test-postgres`) başlatılır ve sonraki çalıştırmalarda yeniden kullanılır. Bir saatten eski test şemaları otomatik silinir. Konteyneri tamamen kaldırmak için: `docker rm -f secondbrainv4-test-postgres`.
- Docker gerekir. **OrbStack kullanıyorsanız** testcontainers Docker soketini kendiliğinden bulamaz, kabuğunuzda şunu tanımlayın: `export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock`.
- Konteyner yerine mevcut bir sunucu kullanmak için: `TEST_DATABASE_URL=postgres://...` (ör. compose veritabanı).

## Sunucuyu çalıştırma
```bash
cargo run -p app        # .env okunur; varsayılan http://127.0.0.1:8080
curl localhost:8080/health
```
Yapılandırma ortam değişkenleriyle yapılır: `DATABASE_URL` (zorunlu), `DB_MAX_CONNECTIONS`, `DB_MIGRATE_ON_START`, `APP_HOST`, `APP_PORT`, `SHUTDOWN_TIMEOUT_SECS` (bkz. `.env.example`). Önce `pnpm db:up` ile yerel veritabanını başlatın. SIGTERM veya Ctrl-C gelince sunucu yeni bağlantı almayı bırakır, devam eden istekleri en fazla `SHUTDOWN_TIMEOUT_SECS` saniye bekler ve çıkar.

## Komutlar
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
