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

## Domain olayları (outbox)
Modüller birbirinin tablosuna dokunmadan haberleşmek için domain olayı yayınlar (ADR-0007, ARCHITECTURE §4.5):

```rust
// 1) Olayı tanımla (kernel::DomainEvent). EVENT_TYPE kalıcıdır, yeniden adlandırılmaz.
#[derive(Serialize, Deserialize)]
struct ItemCreated { item_id: ItemId }
impl DomainEvent for ItemCreated { const EVENT_TYPE: &'static str = "item.created"; }

// 2) Değişiklikle AYNI transaction içinde yayınla
let mut tx = pool.begin().await?;
/* INSERT ... */
db::outbox::publish(&mut tx, &ItemCreated { item_id }).await?;
tx.commit().await?;

// 3) Başka bir modülde abone ol (app::run içinde kaydedilir)
dispatcher.subscribe("search-index", |event: ItemCreated, stored| async move { /* ... */ Ok(()) });
```

- Dağıtıcı `app::run` içinde arka planda çalışır, olayları `FOR UPDATE SKIP LOCKED` ile alır. Birden fazla sunucu örneği güvenle birlikte çalışır.
- **En az bir kez teslim:** Bir abone hata verirse veya süreç yarıda kalırsa olay tekrar teslim edilir. **Aboneler idempotent olmalıdır** (`stored.id` olay kimliğidir, tekrarları ayıklamak için kullanılabilir).
- Hatalı teslimler üstel bekleme (backoff) ile tekrar denenir; maskelenmiş hata `outbox.last_error` alanında tutulur.
- Katı sıra garantisi yoktur: tekrar denenen bir olay daha yenilerinden sonra gelebilir.

## Arka plan işleri (`jobs`)
```rust
#[derive(Serialize, Deserialize, Clone)]
struct SendWeeklySummary { user_id: UserId }
impl jobs::Job for SendWeeklySummary {
    const KIND: &'static str = "summary.weekly"; // kalıcı, yeniden adlandırılmaz
    const MAX_ATTEMPTS: u32 = 5;                 // varsayılan 10
}

// Kuyruğa ekle (çağıranın transaction'ı içinde)
jobs::enqueue(&mut tx, &job, EnqueueOptions { run_at: Some(at), dedupe_key: None }).await?;

// İşleyici (app::run içinde kaydedilir)
worker.register(|job: SendWeeklySummary, claimed| async move { /* ... */ Ok(()) });

// Tekrarlayan iş (6 alanlı cron, UTC): her pazartesi 08:00
scheduler.add("weekly-summary", "0 0 8 * * Mon", SendWeeklySummary { .. })?;
```

- İşçi, vadesi gelen işleri `FOR UPDATE SKIP LOCKED` ile **kira (lease)** altında alır ve işleyiciyi transaction dışında çalıştırır. Kira süresini aşan işleyici iptal edilir ve iş tekrar denenir. Çöken bir işçinin işi kira bitince başka işçiye geçer.
- **En az bir kez çalışır:** İşleyiciler idempotent olmalıdır (`claimed.id` iş kimliğidir).
- Hatalar üstel backoff ile tekrar denenir. `MAX_ATTEMPTS` dolunca iş `failed_at` ile işaretlenir ve bir daha çalışmaz. Maskelenmiş son hata `last_error` alanındadır.
- Zamanlayıcı her tetiklemede her takvimin **bir sonraki** çalışma anını kuyruğa koyar (`cron:<ad>:<an>` benzersiz anahtarıyla). Birden fazla sunucu örneği aynı anı tek kez ekler. Sunucu kapalıyken kaçırılan tetiklemeler topluca çalıştırılmaz.
- Kullanıcının saat dilimine bağlı işler (ör. rutin hatırlatmaları) cron ile değil, `run_at` ile tek tek kuyruğa eklenir.

## Veritabanı testleri
`db::testing::TestDb` (feature `testing`) her test için ayrı bir şema açar, tüm migration'ları o şemada uygular ve o şemaya bağlı bir pool verir:

```rust
// Cargo.toml → [dev-dependencies] db = { path = "../db", features = ["testing"] }
let db = db::testing::TestDb::new().await;
sqlx::query("...").execute(db.pool()).await?;
```

Migration akışının kendisini test etmek için boş bir şema: `TestDb::new_unmigrated()`.

- İlk kullanımda testcontainers ile **tek bir Postgres 17 konteyneri** (`secondbrainv4-test-postgres`) başlatılır ve sonraki çalıştırmalarda yeniden kullanılır. Bir saatten eski test şemaları otomatik silinir. Konteyneri tamamen kaldırmak için: `docker rm -f secondbrainv4-test-postgres`.
- Docker gerekir. **OrbStack kullanıyorsanız** testcontainers Docker soketini kendiliğinden bulamaz, kabuğunuzda şunu tanımlayın: `export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock`.
- Konteyner yerine mevcut bir sunucu kullanmak için: `TEST_DATABASE_URL=postgres://...` (ör. compose veritabanı).

## Kimlik doğrulama (`identity`)
- Uç noktalar: `POST /auth/register`, `POST /auth/login`, `POST /auth/logout`, `GET /auth/sessions`, `DELETE /auth/sessions/{id}` (ayrıntı: `crates/identity/src/http.rs`).
- İstemci gövdede `client` gönderir: `"web"` → token `sb_session` cookie'sine yazılır (`HttpOnly; Secure; SameSite=Lax`), gövdede dönmez. `"native"` → token gövdede bir kez döner, sonra `Authorization: Bearer <token>` ile gönderilir.
- Korumalı bir uç nokta yazarken handler'a `identity::http::Authenticated` extractor'ını ekleyin. Token eksik, bozuk, bilinmeyen veya süresi dolmuşsa 401 döner. Router state'i `IdentityService`'i `FromRef` ile sağlamalıdır.
- Oturumlar 30 gün kullanılmayınca düşer; her kullanım süreyi uzatır (en fazla 5 dakikada bir yazılır).

## Loglama ve hata takibi
- Loglar stdout'a satır başına bir JSON nesnesi olarak yazılır (`LOG_FORMAT=json`, varsayılan). Yerelde `LOG_FORMAT=pretty` daha okunaklıdır. Seviye `RUST_LOG` ile ayarlanır (varsayılan `info,sqlx=warn`).
- Her isteğin bir kimliği vardır (`x-request-id`). İstemci geçerli bir kimlik gönderirse (`[A-Za-z0-9._-]`, en fazla 64 karakter) o kullanılır, yoksa UUIDv7 üretilir. Yanıtta döner ve o isteğin tüm log satırlarına eklenir.
- **Kişisel veri loglanmaz** (ARCHITECTURE §15, KVKK):
  - İstek logunda sadece method, **rota şablonu** (`/items/{id}`, ham yol değil), status, süre ve istek kimliği bulunur. Query string, header'lar (cookie, `Authorization`), istemci IP'si ve gövde **asla** loglanmaz.
  - İç hatalarda (500) hata metni `kernel::redact` ile maskelenir (e-posta, IPv4, Postgres detay değerleri, Bearer token).
  - Yeni log satırı eklerken: kullanıcı adı, e-posta, not içeriği, başlık gibi alanları log alanı olarak yazmayın. Gerekirse id kullanın.
- **Sentry** isteğe bağlıdır (`SENTRY_DSN` boşsa kapalı). Sadece `error` seviyesindeki olaylar gönderilir ve sadece izin listesindeki veriler gider: olay mesajı, `error` alanı ve `method`/`route`/`request_id` etiketleri. `send_default_pii` kapalıdır.

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
