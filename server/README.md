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
- Migration'lar `server/migrations/` altında (klasördeki her değişiklik `db` crate'ini yeniden derletir, bkz. `crates/db/build.rs`), sqlx formatında: `<YYYYMMDDHHMMSS>_<açıklama>.sql`. Derleme sırasında binary'ye gömülür (`db::MIGRATOR`).
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

- **Görünürlük kuralları (V1–V12)** `crates/app/tests/visibility/` altında, kural başına bir dosya. Henüz implemente edilmemiş senaryolar `#[ignore = "T<x.y.z>: …"]` ile açılacağı görevi söyler (meta test bunu ve görevin ROADMAP'te olduğunu denetler). Görevi biten senaryodan `#[ignore]`'u kaldırıp `pending()` yerine gerçek senaryoyu yazın. Bekleyenleri görmek için: `cargo test -p app --test visibility -- --ignored`.

## Idempotency key'leri (`app::idempotency`)
Her mutasyon istemcide üretilen bir `Idempotency-Key` başlığı taşır (ARCHITECTURE §10, ADR-0003).
- Domain modüllerinin router'ları `app::run` içinde `idempotency::protect(..)` içine eklenir. Bu router'larda oturum açmış kullanıcının `POST/PUT/PATCH/DELETE` istekleri başlıksız gelirse 422 döner.
- Aynı kullanıcı + anahtar için ilk yanıt (2xx/4xx) 24 saat saklanır; tekrarında handler çalışmaz, saklanan yanıt `Idempotent-Replayed: true` ile döner. Aynı anahtar farklı istekle gelirse 422, ilk istek sürerken 409. 5xx saklanmaz (istemci aynı anahtarla tekrar deneyebilir). Yarıda kalan bir denemenin kilidi 2 dakika sonra düşer.
- `/auth/*` muaftır: oturum öncesi isteklerdir ve yanıtları token taşır (token asla saklanmaz).
- Sınır: kayıt ile handler'ın kendi transaction'ı ayrıdır. Süreç handler commit'inden hemen sonra çökerse, kilit süresi dolduktan sonraki tekrar yeniden çalışır.
- Eski anahtarlar her gün 03:30 UTC'de `idempotency.cleanup` işiyle silinir.

## Kimlik doğrulama (`identity`)
- Uç noktalar: `POST /auth/register`, `POST /auth/login`, `POST /auth/logout`, `GET /auth/sessions`, `DELETE /auth/sessions/{id}` (ayrıntı: `crates/identity/src/http.rs`).
- İstemci gövdede `client` gönderir: `"web"` → token `sb_session` cookie'sine yazılır (`HttpOnly; Secure; SameSite=Lax`), gövdede dönmez. `"native"` → token gövdede bir kez döner, sonra `Authorization: Bearer <token>` ile gönderilir.
- Korumalı bir uç nokta yazarken handler'a `identity::http::Authenticated` extractor'ını ekleyin. Token eksik, bozuk, bilinmeyen veya süresi dolmuşsa 401 döner. Router state'i `IdentityService`'i `FromRef` ile sağlamalıdır.
- Oturumlar 30 gün kullanılmayınca düşer; her kullanım süreyi uzatır (en fazla 5 dakikada bir yazılır).
- E-posta doğrulama ve şifre sıfırlama: `POST /auth/email/verify`, `POST /auth/email/verification` (yeniden gönder), `POST /auth/password/forgot` (her zaman 202), `POST /auth/password/reset` (tüm oturumları kapatır). Bağlantılar `PUBLIC_WEB_URL/verify-email?token=…` ve `…/reset-password?token=…` biçimindedir; doğrulama 24 saat, sıfırlama 1 saat geçerlidir.

- Google ile giriş (T0.3.4): web'de `GET /auth/google/start` → Google → `GET /auth/google/callback` (sunucu kodu değiştirir, cookie'yi yazar, web uygulamasına yönlendirir). Mobilde uygulama PKCE ile kodu kendisi alır ve `POST /auth/google/native`'e gönderir. `GOOGLE_CLIENT_ID`/`GOOGLE_CLIENT_SECRET` boşsa Google uç noktaları 404 döner. Hesap bağlama kuralları `crates/identity/src/oauth/mod.rs` başındadır (doğrulanmamış hesaba bağlanırken eski şifre ve oturumlar silinir).

- Apple ile giriş (T0.3.5): web'de `GET /auth/apple/start` → Apple → `POST /auth/apple/callback` (Apple `form_post` ile döner; bu yüzden state cookie'si `SameSite=None`). iOS'ta uygulama `authorizationCode` ve ilk girişte adı `POST /auth/apple/native`'e gönderir. Apple adı sadece ilk yetkilendirmede gönderir; gizli (private relay) adresler normal doğrulanmış adres gibi saklanır. Token uç noktası için `client_secret`, `.p8` anahtarıyla her istekte imzalanan 5 dakikalık bir ES256 JWT'dir. `APPLE_*` boşsa Apple uç noktaları 404 döner.

- **CSRF (T0.3.6):** cookie ile kimliği doğrulanan `POST/PUT/PATCH/DELETE` istekleri `X-CSRF-Token` başlığı taşımak zorunda (yoksa 403). Token oturum token'ından türetilir; web girişinde `AuthResponse.csrf_token` ile, sayfa yenilenince `GET /auth/csrf` ile alınır. Bearer isteklerinde gerekmez. Kimlik doğrulama gereken her yerde `identity::http::authenticate_request` (veya `Authenticated` extractor'ı) kullanın; CSRF kontrolü oradadır.
- **Hız sınırları (T0.3.6):** kimlik bilgisi kontrol eden uç noktalar (kayıt, giriş, sıfırlama, doğrulama, Google/Apple native) IP başına 10 istek, sonra 6 saniyede bir; e-posta gönderenler (forgot, yeniden gönder) 3 istek, sonra dakikada bir. Aşımda 429 + `Retry-After`. Sınırlar sunucu örneği başına bellektedir; IP loglanmaz. Uygulama `identity::http::rate_limited_router` kullanır (`router` sınırsızdır, testler için). Ters proxy arkasında `TRUST_PROXY_HEADERS=true` (yoksa tüm istemciler proxy'nin IP'sini paylaşır); proxy yokken açmayın (istemci kendi IP'sini seçebilir).
- **CORS (T0.4.9, T0.6.4):** API'yi tarayıcıdan (cookie ile) çağırabilen origin'ler `PUBLIC_WEB_URL`'in origin'i ve `CORS_EXTRA_ORIGINS` listesidir. Joker, yol ve kimlik bilgisi içeren girişler reddedilir. Aynı liste `/ws` cookie el sıkışmasındaki `Origin` kontrolünde de kullanılır. `CORS_EXTRA_ORIGINS` http/https dışında yalnız Tauri masaüstü kabuğunun (`apps/desktop`, ADR-0020) `tauri://` şemasını kabul eder. Masaüstü için ekleyin: macOS/Linux `tauri://localhost`, Windows `http://tauri.localhost` (yayın paketinde `https://tauri.localhost` olabilir).

## E-posta (`kernel::email`)
- Modüller e-postayı `EmailSender` arayüzüyle gönderir; sağlayıcıyı bilmez. Sağlayıcı Resend'dir (ADR-0022).
- `EMAIL_DRIVER=resend`: `ResendEmailSender` Resend HTTP API'sine (`POST /emails`) düz metin e-posta gönderir. `RESEND_API_KEY` (sır) ve `EMAIL_FROM` (Resend'de doğrulanmış alan adında bir adres) zorunludur; eksikse sunucu açılmaz. Hata mesajlarında sadece HTTP durum kodu bulunur; anahtar, alıcı ve gövde loglanmaz. Gönderim başarılıysa `email` hedefinde sadece türü loglanır.
- `EMAIL_DRIVER=log` (varsayılan): e-posta gönderilmez, konu ve gövde (bağlantı dahil) `email` hedefiyle loga yazılır. Alıcı adresi yazılmaz. Yerelde doğrulama bağlantısını logdan kopyalayabilirsiniz. **`APP_ENV=production` ise sunucu bu sürücüyle açılmaz.**
- Testlerde `kernel::email::RecordingEmailSender` gönderilen mesajları toplar.
- E-posta metinlerine ad gibi kişisel veri koymayın; `EmailMessage`'ın `Debug` çıktısı alıcıyı ve gövdeyi gizler.

## Loglama ve hata takibi
- Loglar stdout'a satır başına bir JSON nesnesi olarak yazılır (`LOG_FORMAT=json`, varsayılan). Yerelde `LOG_FORMAT=pretty` daha okunaklıdır. Seviye `RUST_LOG` ile ayarlanır (varsayılan `info,sqlx=warn`).
- Her isteğin bir kimliği vardır (`x-request-id`). İstemci geçerli bir kimlik gönderirse (`[A-Za-z0-9._-]`, en fazla 64 karakter) o kullanılır, yoksa UUIDv7 üretilir. Yanıtta döner ve o isteğin tüm log satırlarına eklenir.
- **Kişisel veri loglanmaz** (ARCHITECTURE §15, KVKK):
  - İstek logunda sadece method, **rota şablonu** (`/items/{id}`, ham yol değil), status, süre ve istek kimliği bulunur. Query string, header'lar (cookie, `Authorization`), istemci IP'si ve gövde **asla** loglanmaz.
  - İç hatalarda (500) hata metni `kernel::redact` ile maskelenir (e-posta, IPv4, Postgres detay değerleri, Bearer token).
  - Yeni log satırı eklerken: kullanıcı adı, e-posta, not içeriği, başlık gibi alanları log alanı olarak yazmayın. Gerekirse id kullanın.
- **Sentry** isteğe bağlıdır (`SENTRY_DSN` boşsa kapalı). Sadece `error` seviyesindeki olaylar gönderilir ve sadece izin listesindeki veriler gider: olay mesajı, `error` alanı ve `method`/`route`/`request_id` etiketleri. `send_default_pii` kapalıdır.

## Gerçek zamanlı kanal (`/ws`, T0.5.1–T0.5.2, T0.5.6)
- `realtime` crate'i: `GET /ws` el sıkışması diğer istekler gibi doğrulanır (`Authorization: Bearer` ya da web'de oturum cookie'si). Cookie ile gelen el sıkışmada `Origin` izinli web origin'lerinden (CORS listesi) biri olmalıdır; aksi 403 (cross-site WebSocket hijacking).
- Mesajlar JSON, `type` alanıyla etiketli; tipler `packages/api-types` içinde (`ClientMessage`, `ServerMessage`). Sunucu ilk olarak `hello` ile kendi saatini gönderir. İstemci `subscribe`/`unsubscribe` ile kanallara abone olur (şimdilik yalnız `user:<kendi id>`), 25 sn'de bir `ping` gönderir; 60 sn sessiz kalan bağlantı 4000 koduyla kapanır.
- WS veri taşımaz, sadece "değişti" der (`event {channel, event: {name, data}}`). Yavaş tüketici (64 mesajlık tampon dolarsa) düşürülür ve 4001 koduyla kapanır; istemci yeniden bağlanıp tazeler.
- **Olay yayınlama (T0.5.2, ADR-0009):** Modüller `Hub::publish`'i doğrudan çağırmaz, `realtime::Notifier` kullanır. Notifier Postgres `NOTIFY` gönderir; her sunucu örneği (gönderen dahil) `app::run` içinde başlatılan `realtime::Bridge` ile `LISTEN` eder ve olayı kendi hub'ına verir. Böylece bağlantı hangi örnekteyse olay oraya ulaşır.

```rust
let mut tx = pool.begin().await?;
/* UPDATE ... */
notifier.publish(&mut *tx, &Channel::User(owner), &Event { name: "item.changed".into(), data: json!({"id": id, "kind": "task"}) }).await?;
tx.commit().await?; // NOTIFY commit'te gider; rollback'te hiç gitmez
```

- Payload en fazla 7900 bayt (Postgres sınırı 8000); olaylar sadece id taşır. Teslim "en iyi çaba"dır: dinleyici bağlantısı koptuğu sırada gönderilen olaylar kaybolur (köprü kendiliğinden yeniden bağlanır ve uyarı loglar). İstemci zaten yeniden bağlanınca her şeyi tazeler.
- **Oturum bitince (T0.5.6):** Çıkış, `DELETE /auth/sessions/{id}` ve şifre sıfırlama, aynı transaction içinde `identity.sessions_ended` outbox olayını yayınlar. Olayı realtime'ın aboneliği (`realtime::sessions::register`) alır; dağıtıcı olayı tek bir örnekte işlediği için abonelik olayı NOTIFY ile tüm örneklere iletir. Her örnek o oturuma ait bağlantıları **4003** koduyla kapatır. Süresi dolan ya da bildirimi kaçırılan oturumlar her `ping`'de (25 sn'de bir) kontrol edilir ve aynı kodla kapatılır.
- Kapatma kodları: 4000 sessizlik zaman aşımı, 4001 yavaş tüketici (bu ikisinde yeniden bağlanılır), 4003 oturum bitti (yeniden giriş gerekir).
- Testlerde bildirim kanalları veritabanı geneli olduğundan her test kendi şema adını kanal adı olarak kullanır (`Bridge::start(pool, db.schema(), hub)`).
- Nesne ve takım kanalları eklendiğinde yetki `PermissionService` üzerinden verilecek.

## Sunucuyu çalıştırma
```bash
cargo run -p app        # .env okunur; varsayılan http://127.0.0.1:8080
curl localhost:8080/health
```
En düşük desteklenen istemci sürümü (T4.2.7): `GET /client/policy` (herkese açık) platform başına `MIN_VERSION_*` ve `DOWNLOAD_URL_*` değerlerini döner. Daha eski masaüstü/iOS/Android uygulamaları "yeni sürüm gerekli" ekranında kalır.

Mobil OTA güncellemeleri (`updates` crate, T4.2.6): `UPDATES_DIR` tanımlıysa `GET /updates/manifest` ve `GET /updates/assets/...` açılır (Expo Updates protokolü v1, sadece dosya; bkz. `apps/client/BETA.md`).

Yapılandırma ortam değişkenleriyle yapılır: `DATABASE_URL` (zorunlu), `DB_MAX_CONNECTIONS`, `DB_MIGRATE_ON_START`, `APP_HOST`, `APP_PORT`, `SHUTDOWN_TIMEOUT_SECS` (bkz. `.env.example`). Önce `pnpm db:up` ile yerel veritabanını başlatın. SIGTERM veya Ctrl-C gelince sunucu yeni bağlantı almayı bırakır, devam eden istekleri en fazla `SHUTDOWN_TIMEOUT_SECS` saniye bekler ve çıkar.

Canlı ortam için sunucu imajı `server/Dockerfile` ile derlenir. Sunucu yapısı ve deploy için bkz. [infra/production/README.md](../infra/production/README.md) (ADR-0021).

## Komutlar
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Ortak düzenleme spike'ı (`/collab/{page_id}`, T0.6.2)
- `notebooks::collab`: yrs ile y-sync (ADR-0019). Sayfa başına bellekte bir belge; yetki ve kalıcılık **yok** (E2.5).
- Sadece `COLLAB_SPIKE_ENABLED=true` iken bağlanır (varsayılan kapalı; açılınca açılışta uyarı loglanır). Staging ve üretimde açmayın.
- İstemci: web'de `y-websocket` doğrudan; mobilde React Native soketi açar, WebView'daki editöre çerçeveleri aktarır (`apps/client/src/lib/editor`). Mobil editör paketi: `pnpm --filter @secondbrain/client editor:build`.
