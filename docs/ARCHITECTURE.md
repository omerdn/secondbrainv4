# Mimari Doküman

> Sürüm: 1.0 · Tarih: 2026-11-21
> Gereksinimler: [PRD.md](PRD.md) · Kararların gerekçeleri: [DECISIONS.md](DECISIONS.md)
> Bu doküman yaşayan bir dokümandır. Mimari bir karar değiştiğinde önce DECISIONS.md'ye yeni bir ADR eklenir, sonra bu doküman güncellenir.

---

## 1. Genel bakış

```
┌────────────────────────────── İstemci (tek kod tabanı) ──────────────────────────────┐
│  Expo (React Native) + react-native-web                                              │
│   ├─ iOS / Android  → native build (Expo)                                            │
│   ├─ Web            → expo export (statik web)                                       │
│   └─ Masaüstü       → aynı web çıktısı, Tauri 2 (Rust) kabuğunda                     │
└───────────────┬───────────────────────────────┬──────────────────────────────────────┘
                │ HTTPS (JSON REST)             │ WebSocket
                ▼                               ▼
┌────────────────────────── Sunucu: Rust modüler monolit ──────────────────────────────┐
│  axum HTTP API  │  /ws gerçek zamanlı kanal  │  /collab/:page Yjs (yrs) senkronu      │
│  Modüller: identity · access · items · tracking · recurrence · tasks · routines ·    │
│            goals · notebooks · sessions · social · teams · notifications · search    │
│  Arka plan işleri (Postgres tabanlı kuyruk)                                          │
└───────────────┬──────────────────────────────────────────────────────────────────────┘
                ▼
         PostgreSQL 17 (tek veritabanı; LISTEN/NOTIFY, tam metin arama, iş kuyruğu)
```

## 2. Teknoloji yığını

| Katman | Seçim | Not |
|---|---|---|
| Dil (istemci) | TypeScript (strict) | |
| Arayüz | Expo (güncel SDK) + Expo Router + react-native-web | Tek kod tabanı. Web'de geliştirilir, mobilde periyodik olarak doğrulanır. |
| Stil | NativeWind (Tailwind) + kendi bileşen kütüphanemiz (`packages/ui`) | Tema token'ları; takım renkleri CSS değişkeni ve tema olarak verilir. |
| Sunucu durumu | TanStack Query | WebSocket olaylarıyla geçersiz kılınır (invalidation). |
| UI durumu | Zustand | Sadece yerel UI durumu için. Sunucu verisi burada tutulmaz. |
| Listeler | FlashList | |
| Editör | TipTap (ProseMirror) + Yjs. Mobilde TipTap, WebView içinde çalışır (10tap-editor). | |
| Masaüstü | Tauri 2 | Web build'ini paketler. Rust tarafı ileride bildirim, kısayol ve otomatik güncelleme için kullanılır. |
| Sunucu | Rust (stable), tokio, axum, tower | |
| Veritabanı erişimi | sqlx | Derleme zamanında kontrol edilen SQL. Migration'lar sqlx-cli ile. |
| Veritabanı | PostgreSQL 17 | `pg_trgm`, `unaccent`, Türkçe tam metin arama. |
| CRDT | Yjs (istemci) + yrs (sunucu) | y-sync protokolü. |
| Tekrar kuralları | RFC 5545 RRULE (`rrule` crate) | |
| Tip paylaşımı | `ts-rs` | Rust DTO'larından TypeScript tipleri üretilir: `packages/api-types`. |
| Kimlik doğrulama | Kendi implementasyonumuz | argon2id; opak oturum token'ı (veritabanında hash'li saklanır); OAuth (Google, Apple) için `oauth2` crate. |
| Arka plan işleri | Postgres tablosu + `FOR UPDATE SKIP LOCKED` | Redis yok (ADR-0009). |
| Gözlemlenebilirlik | `tracing` + OpenTelemetry | Hata takibi için Sentry. |
| Test | Rust: `cargo test`, `proptest`, testcontainers. TS: Vitest (saf TS paketleri), jest-expo + React Native Testing Library (RN bileşenleri). E2E: Playwright (web). | Vitest, React Native'in Flow kaynaklarını işleyemez; RN tarafında Expo'nun resmî Jest preset'i kullanılır. |
| Monorepo | pnpm workspaces + Turborepo (TS) · Cargo workspace (Rust) | |
| CI | GitHub Actions | |
| Dağıtım (staging ve beta) | Docker Compose: tek VPS + Caddy (TLS) + Postgres | Yedekler S3 uyumlu depoya alınır. |

## 3. Repo yapısı

```
/
├─ apps/
│  ├─ client/            # Expo uygulaması (iOS, Android, web)
│  │  ├─ app/            # Expo Router rotaları
│  │  └─ src/
│  │     ├─ features/    # özellik klasörleri: tasks/, routines/, goals/, notebooks/, sessions/...
│  │     ├─ connectivity/# bağlantı katmanı (BP1–BP5)
│  │     └─ lib/
│  └─ desktop/           # Tauri 2 kabuğu (src-tauri/)
├─ packages/
│  ├─ ui/                # tasarım sistemi (RN bileşenleri, token'lar, Tailwind preset)
│  ├─ api-types/         # ts-rs ile üretilen tipler (elle düzenlenmez)
│  ├─ api-client/        # tipli HTTP + WS istemcisi, TanStack Query hook'ları
│  └─ config/            # paylaşılan eslint, prettier, tsconfig
├─ server/               # Cargo workspace
│  ├─ crates/
│  │  ├─ app/            # binary: config, router birleştirme, sunucu başlatma
│  │  ├─ kernel/         # ortak tipler: Id'ler, hata, saat (Clock), domain olay veriyolu, sayfalama
│  │  ├─ db/             # bağlantı havuzu, migration'lar, test yardımcıları
│  │  ├─ identity/       # kullanıcılar, kimlik doğrulama, oturumlar, arkadaşlık, engelleme
│  │  ├─ access/         # ilişki tuple'ları, erişim hesaplama, PermissionService
│  │  ├─ items/          # nesne tabanı, etiketler, bağlantılar, kayıtlı görünümler, aktivite
│  │  ├─ tracking/       # takip motoru: takipler, kayıtlar, ProgressProvider
│  │  ├─ recurrence/     # RRULE, tekrar üretimi, plan sürümleri (saf kütüphane)
│  │  ├─ tasks/          # görevler, alt kırılımlar, kişisel kanban
│  │  ├─ routines/       # rutinler, tamamlama modları, rotasyon, streak
│  │  ├─ goals/          # hedefler, ağırlıklar, ilerleme hesabı
│  │  ├─ notebooks/      # defterler, sayfalar, collab sunucusu (yrs)
│  │  ├─ sessions/       # Work Session
│  │  ├─ social/         # dürtme, emoji tepkileri
│  │  ├─ teams/          # takımlar, roller, yetki matrisi
│  │  ├─ notifications/  # bildirim merkezi, push, e-posta, tercihler
│  │  ├─ search/         # global arama
│  │  ├─ realtime/       # WS hub, kanal abonelikleri, LISTEN/NOTIFY köprüsü
│  │  └─ jobs/           # iş kuyruğu çalıştırıcısı, zamanlanmış işler
│  └─ migrations/
├─ docs/                 # PRD, ARCHITECTURE, DECISIONS, ROADMAP
├─ .github/workflows/
├─ CLAUDE.md
└─ README.md
```

## 4. Modül sınırları (spagetti kodu önleyen kurallar)

1. **Her domain modülü bir crate'tir.** Dışarıya sadece `pub` servis arayüzlerini, DTO'larını ve olay tiplerini açar.
2. **Bir modül başka bir modülün tablolarına doğrudan SQL ile erişemez.** Başka bir modülden veri gerekiyorsa, o modülün servis arayüzü çağrılır ya da onun yayınladığı bir domain olayına abone olunur.
3. **Bağımlılık yönü tek yönlüdür:**
   ```
   kernel ← db ← access ← items ← tracking ← {tasks, routines, goals, notebooks, sessions}
                      ↑                           ↑
                   identity                     teams
   ```
   Döngüsel bağımlılık Cargo tarafından zaten engellenir. Daha üst katmanlar arasındaki iletişim domain olaylarıyla yapılır.
4. **Saf mantık ile yan etkiler ayrılır.** Erişim hesaplama, tekrar üretimi, ilerleme, streak ve tempo hesapları **saf fonksiyonlardır**. Veritabanı veya saate dokunmazlar; `Clock` dışarıdan enjekte edilir. Bu sayede `proptest` ile yoğun şekilde test edilirler.
5. **Domain olayları:** `ItemCreated`, `LinkAdded`, `MemberAdded`, `EntryRecorded`, `SubtaskToggled`… gibi olaylar, aynı veritabanı işlemi (transaction) içinde `outbox` tablosuna yazılır. Bir dağıtıcı bu olayları abonelere ve WebSocket hub'ına iletir. Böylece olaylar kaybolmaz.
6. **İstemcide de aynı kural:** `features/x` klasörü `features/y`'nin iç dosyalarını import edemez; sadece public girişini (`@/features/y`, yani `index.ts`) kullanabilir. Paylaşılan kod `packages/*` veya `src/lib` altında durur ve hiçbir feature'ı import etmez. Bu kural `@secondbrain/config` içindeki ESLint kuralı `secondbrain/feature-boundaries` ile zorlanır (göreli yolları da çözer; `features/modules/<ad>` her biri ayrı feature sayılır).
7. Ek modüller (§13) çekirdeğe sadece `ModuleManifest` ve `ProgressProvider` arayüzleri üzerinden bağlanır.

## 5. Veri modeli (çekirdek)

> Tüm id'ler UUIDv7, tüm zaman damgaları `timestamptz` (UTC). "Yerel gün" gerektiren alanlar `date` tipindedir ve ilgili saat dilimiyle birlikte saklanır.

### 5.1 Kimlik ve sosyal
- `users(id, email, display_name, avatar_url, timezone, locale, created_at, deletion_requested_at)`
- `auth_identities(user_id, provider[password|google|apple], provider_uid, password_hash)`
- `sessions(id, user_id, token_hash, device, created_at, last_seen_at, expires_at)`
- `friend_requests(id, from_user, to_user, status[pending|accepted|declined], created_at, responded_at)`
- `friendships(user_a, user_b, created_at)`: `user_a < user_b` sırasıyla tek satır.
- `blocks(blocker, blocked, created_at)`

### 5.2 Takımlar
- `teams(id, name, color, logo_url, created_by, created_at)`
- `team_members(team_id, user_id, role[admin|manager|member|guest], status[invited|active], joined_at)`
- `team_role_permissions(team_id, role, permission, allowed)`: Admin tarafından düzenlenen yetki matrisi. Varsayılan değerler sabit kodda tutulur.

### 5.3 Nesneler
- `items(id, kind, owner_user_id NULL, owner_team_id NULL, title, description, status, team_visibility[all|assignees] NULL, created_by, created_at, updated_at, last_touched_at, archived_at, deleted_at)`
  - `kind` değerleri: `task | routine | goal | tracker | notebook | page | module:<ad>`
  - `CHECK`: `owner_user_id` ve `owner_team_id` alanlarından tam olarak biri dolu olmalı.
  - `last_touched_at`: "Unutulanlar" (H4) bu alana göre hesaplanır.
- `item_tags(item_id, tag_id, tagged_by)` · `tags(id, owner_user_id NULL, owner_team_id NULL, name, color)`
- `item_links(id, source_item_id, target_item_id, created_by, created_at)`
  - Örnek yönler: sayfa → nesne, nesne → hedef.
- `saved_views(id, user_id, name, filter jsonb, pinned, position)`
- `activity_events(id, actor_id, item_id NULL, team_id NULL, type, payload jsonb, created_at)`
- `invitations(id, kind[item_member|team_member], object_id, from_user, to_user, role, status, created_at)`
- `outbox(id, event_type, payload jsonb, created_at, dispatched_at)`

Her türün kendi detay tablosu vardır: `tasks`, `routines`, `goals`, `trackers`, `notebooks`, `pages`… Bu tablolar `item_id` üzerinden `items` tablosuna bağlanır.

### 5.4 Görevler
- `tasks(item_id, start_date, due_date)`
- `subtasks(id, task_id, title, position, done_at, done_by)`
- `subtask_assignees(subtask_id, user_id)`
- `user_board_columns(id, user_id, name, position, is_inbox)`: Her kullanıcının bir gelen kutusu sütunu (`is_inbox`) vardır (G6).
- `user_task_placements(user_id, task_id, column_id, position)`: `position`, kesirli indeks (fractional indexing) ile tutulur.

### 5.5 Takip motoru
- `trackers(item_id, unit, target_value, period[day|week|month|total|range], aggregation[sum|count|last|max], direction[increase|limit], owner_kind[standalone|routine|module])`
- `tracker_entries(id, tracker_item_id, user_id, value numeric, occurred_at, local_date, note_block_id NULL, created_at, deleted_at)`
  - `note_block_id`: kaydın notunu süreç sayfasındaki satıra bağlar (T3).

### 5.6 Rutinler
- `routines(item_id, timezone, daily_target int default 1, completion_mode[pool|individual], assignment_mode[fixed|rotation], start_date, end_date, reminder_times time[])`
- `routine_schedules(id, routine_id, rrule text, valid_from date, valid_to date NULL)`: Plan sürümleri (R11).
- `routine_rotation(routine_id, user_id, position)` · `routine_rotation_overrides(routine_id, occurrence_date, user_id)`: Takaslar.
- Tamamlamalar `tracker_entries` tablosunda tutulur. Rutinin bir takibi vardır (`owner_kind = routine`). Bir tekrar, `local_date` ile eşleştirilir.

### 5.7 Hedefler
- `goals(item_id, start_date, end_date)`
- `goal_links(goal_id, item_id, weight numeric default 1)`

### 5.8 Defterler
- `notebooks(item_id, is_default)`: Sahibi, `items` tablosundan gelir (kişi veya takım).
- `pages(item_id, notebook_id, title, process_of_item_id NULL)`: `process_of_item_id` doluysa bu sayfa o nesnenin süreç sayfasıdır.
- `page_updates(id, page_id, update bytea, created_at)`: Yjs güncellemeleri, sadece ekleme şeklinde (append-only).
- `page_snapshots(page_id, version, state bytea, plain_text, created_at)`: Sıkıştırma ve sürüm geçmişi. `plain_text` arama için kullanılır.

### 5.9 Work Session
- `work_sessions(id, created_by, status[open|closed], is_shared, timer_state[running|paused|stopped], timer_mode, timer_started_at, timer_accumulated_ms, timer_planned_ms, pomodoro_config jsonb, created_at, closed_at)`
- `work_session_participants(session_id, user_id, joined_at, left_at)`
- `work_session_segments(id, session_id, user_id, mode[pomodoro|stopwatch|break], item_id NULL, started_at, ended_at, active_ms)`

### 5.10 Sosyal ve bildirim
- `nudges(from_user, to_user, local_date)`: `UNIQUE(from_user, to_user, local_date)` kısıtı AK3 kuralını sağlar.
- `reactions(id, user_id, target_kind, target_id, emoji, created_at)`
- `notifications(id, user_id, type, payload jsonb, read_at, created_at)` · `notification_prefs(user_id, type, channels)`
- `push_tokens(user_id, platform, token, created_at)`

### 5.11 Yetki
- `access_relations(...)`: §6'da anlatılıyor.
- `item_access(...)`: §6'da anlatılıyor.

## 6. Yetki modeli

### 6.1 İlişkiler (tek doğruluk kaynağı)
Erişimi etkileyen tüm ilişkiler, Zanzibar tarzında tek bir tabloda tutulur:

```
access_relations(object_type, object_id, relation, subject_type, subject_id, created_at)

item:T1      owner      user:ali
item:T1      assignee   user:ayse
item:T1      watcher    user:can
item:T1      owned_by   team:X
team:X       manager    user:mehmet
item:P3      in_notebook item:N1        (sayfa → defter)
item:N1      editor     user:ayse       (defter üyeliği)
item:P3      linked_to  item:T1         (sayfa → nesne bağlantısı)
```

- Domain modülleri bu tabloya **sadece `access` crate'inin API'si** üzerinden yazar: `grant`, `revoke`, `link`, `unlink`.
- Davetler (`invitations`), kabul edilene kadar ilişki oluşturmaz.

### 6.2 Türetilmiş erişim önbelleği
```
item_access(item_id, user_id, perms bitmask, reasons jsonb)
perms: VIEW | EDIT | MANAGE(paylaş/bağla/üye yönet) | DELETE
```
- Listeleme, filtreleme ve arama sorguları `JOIN item_access ON user_id = $me` ile yapılır. Bu sayede hızlıdır ve her yerde aynı kural uygulanır.
- `reasons` alanı, "Bunu neden görüyorum?" (V11) sorusunun cevabını tutar.

### 6.3 Kurallar (saf fonksiyon: `compute_access(graph, item) -> Map<User, Perms>`)
| Kaynak | Verilen yetki |
|---|---|
| Kişisel sahip | VIEW, EDIT, MANAGE, DELETE |
| Takım nesnesi: sahip takım | Rol matrisine göre. `team_visibility = assignees` ise VIEW yetkisi sadece oluşturana, atananlara, izleyicilere ve yönetici ile üstündekilere verilir (V12). |
| Sorumlu / izleyici | VIEW + EDIT / VIEW |
| Defter üyesi (editor / viewer) | Defterdeki sayfalara VIEW + EDIT / VIEW |
| Sayfa `linked_to` nesne | O nesneyi VIEW edebilen herkese sayfada **VIEW + EDIT** verilir. MANAGE ve DELETE asla verilmez (V3). |

- **Bağlama işleminin ön koşulu** (V4): Bağlayan kişinin sayfada MANAGE, hedef nesnede VIEW yetkisi olmalı.
- **Üye ekleme** (V5): MANAGE yetkisi gerekir.
- **Gizli bağlantı** (V9): Bir nesnenin bağlantıları listelenirken her bağlantının karşı ucu `item_access` ile filtrelenir. Kullanıcının göremediği uçlar sorgu sonucundan tamamen çıkarılır; sayıları dahi döndürülmez.

### 6.4 Yeniden hesaplama
1. Bir ilişki değiştiğinde, etkilenen nesne kümesi hesaplanır:
   - değişen nesnenin kendisi,
   - o nesneye `linked_to` ile bağlı sayfalar,
   - nesne bir defterse, defterdeki tüm sayfalar,
   - değişiklik bir takım rolündeyse, takımın tüm nesneleri.
2. Küçük kümeler (yaklaşık 500 nesneye kadar) **aynı transaction içinde** yeniden hesaplanır.
3. Büyük kümeler (ör. takım rol değişikliği) için önce kısıtlayıcı yönde anında güncelleme yapılır (erişim kaldırma her zaman senkron), sonra genişleme bir iş kuyruğu görevi olarak tamamlanır.
4. Erişimi düşen kullanıcılar için `AccessRevoked` olayı yayınlanır. Realtime ve collab katmanları bu olayla açık bağlantıları keser (V10).

### 6.5 Önizleme (V7)
`POST /access/preview` uç noktası, bir eylemin (bağla, üye ekle) **sonucunda erişim kazanacak** kullanıcıların listesini döndürür. Bu hesap, aynı saf fonksiyonun bir "ya şöyle olursa" (what-if) çağrısıdır. İstemci, onay diyaloğunu bu listeyle gösterir.

### 6.6 Gelecek: Zanzibar geçişi
Tüm yetki kontrolleri `PermissionService` arayüzünden geçer: `check`, `list_accessible`, `explain`, `preview`. `access_relations` tablosu zaten tuple formatında olduğu için SpiceDB'ye geçiş, tuple'ların kopyalanması ve bu arayüzün arkasındaki implementasyonun değiştirilmesinden ibarettir (bkz. ADR-0004).

## 7. Takip motoru ve ilerleme hesapları

### 7.1 ProgressProvider
İlerlemeye katkı veren her nesne türü şu arayüzü uygular:
```rust
trait ProgressProvider {
    /// [range] aralığı için: toplam beklenen, gerçekleşen, bugüne kadar beklenen
    fn progress(&self, item: ItemId, range: DateRange, today: LocalDate) -> Progress;
}
struct Progress { done: f64, total: f64, expected_to_date: f64 }
```

### 7.2 Türlere göre hesap
- **Görev:** `total = alt kırılım sayısı`, `done = tamamlanan alt kırılım`. `expected_to_date`, görevin başlangıç ve bitiş tarihleri arasında doğrusal olarak hesaplanır. Görevin tarihleri yoksa hedefin aralığı kullanılır.
- **Rutin:** Aralık = hedef aralığı ∩ rutin aralığı (D5).
  - `total = Σ (aralıktaki her planlı tekrar) × günlük hedef × (individual modda sorumlu sayısı)`
  - `done = Σ min(o tekrardaki gerçekleşen, o tekrarın hedefi)`. Bir günde hedefin üstüne çıkmak başka bir günün eksiğini kapatmaz. Kısmi günler sayılır (R4).
  - `expected_to_date`, bugüne kadarki planlı tekrarların toplam hedefidir.
- **Takip (artmalı):** `done = min(toplanmış değer, hedef)`, `total = hedef`. `expected_to_date` doğrusal hesaplanır.
- **Takip (sınırın altında kalmalı):** Uyum oranı = sınır içinde kalan dönem sayısı / geçen dönem sayısı. Henüz bitmemiş dönem, sınır aşılmadıysa uyumlu sayılır.

### 7.3 Hedef
```
r_i = clamp(done_i / total_i, 0, 1)            # nesnenin gerçekleşen oranı
e_i = clamp(expected_to_date_i / total_i, 0, 1) # nesnenin beklenen oranı
R   = Σ w_i·r_i / Σ w_i                        # Gerçekleşen (D4)
E   = Σ w_i·e_i / Σ w_i
Tempo = E > 0 ? R / E : 1                       # 1.0 = planda
```
- `total_i = 0` olan nesneler hesaba katılmaz ve arayüzde "henüz başlamadı" etiketiyle gösterilir.
- Hesap sonucunun yanında her nesnenin katkı dökümü de döner (D6).

### 7.4 Streak (R9)
Rutinin planlı tekrarları sondan başa doğru taranır. **Tam** tamamlanan her tekrar seriyi 1 artırır, ilk eksik tekrarda tarama durur. Bugünün tekrarı henüz tamamlanmadıysa atlanır, seriyi bozmaz. Modlara göre:
- `pool` modunda: ortak sayaç hedefe ulaştıysa tekrar tamam sayılır.
- `individual` modunda: tüm sorumlular kendi hedefine ulaştıysa tekrar tamam sayılır. Seri ortaktır (R10).

### 7.5 Geriye dönük işaretleme (R7)
Bir kayıt için şu koşul aranır: `local_date ≥ bugün(rutin saat dilimi) − 3 gün` ve `local_date ≤ bugün`. Bu kontrol sunucu tarafında yapılır.

## 8. Tekrar motoru (`recurrence` crate)
- Saf bir kütüphanedir. Girdi: plan sürümleri (`rrule`, `valid_from`, `valid_to`), saat dilimi ve tarih aralığı. Çıktı: tekrar tarihleri listesi.
- Tekrarlar veritabanında **saklanmaz**, ihtiyaç anında hesaplanır. Sadece tamamlamalar (kayıtlar) saklanır.
- "Ayın 31'i" gibi bazı aylarda olmayan günler RFC'ye uygun şekilde atlanır. "Ayın son günü" için `BYMONTHDAY=-1` kullanılır.
- Rotasyon: Tekrarın sorumlusu = `rotation[(tekrar sırası) mod n]`. Takas kayıtları (`overrides`) bu hesaptan önce uygulanır.

## 9. Defterler: gerçek zamanlı düzenleme
- İstemci, `/collab/:page_id` adresine bir WebSocket açar. Bağlantı kurulurken oturum doğrulanır ve sayfada VIEW yetkisi kontrol edilir. EDIT yetkisi yoksa bağlantı salt okunur modda açılır.
- Sunucuda yrs ile y-sync protokolü konuşulur. Gelen her güncelleme önce `page_updates` tablosuna yazılır, ardından diğer bağlı istemcilere iletilir.
- Sıkıştırma: 200 güncellemede bir veya 10 dakikada bir, bir iş kuyruğu görevi güncellemeleri birleştirerek bir `page_snapshots` kaydı oluşturur. Bu sırada arama için `plain_text` de çıkarılır.
- Sürüm geçmişi snapshot'lardan oluşur. Eski bir sürüme dönmek, o sürümün içeriğini **yeni bir güncelleme olarak** uygulamaktır; geçmiş silinmez.
- **Süreç sayfasına sunucudan satır ekleme (T3):** Bir kayıt notla birlikte girildiğinde, `notebooks` modülü sunucu tarafında yrs ile sayfaya tarihli bir paragraf ekler. Paragrafın blok id'si kayıttaki `note_block_id` alanına yazılır.
- **Erişim düşerse:** `AccessRevoked` olayı gelince ilgili collab bağlantısı kapatılır (V10).
- **Editör şeması sürümlüdür.** Şema değişiklikleri için dönüştürme (migration) fonksiyonları yazılır.

## 10. Gerçek zamanlı kanal ve bağlantı politikası

### 10.1 `/ws` kanalı
- Her istemcinin bir WS bağlantısı vardır. Bağlantı kurulurken sunucu saati gönderilir ve istemci kendi saatiyle arasındaki farkı hesaplar (Work Session için).
- Sunucu, istemciye `item.changed {id, kind}`, `notification.new`, `session.timer` gibi olaylar iter. İstemci bu olaylarla ilgili TanStack Query sorgularını geçersiz kılar ve veriyi tazeler. **Veri WS üzerinden taşınmaz; WS sadece "bu değişti" bilgisi verir** (Work Session timer durumu bunun istisnasıdır).
- Birden fazla sunucu örneği çalıştığında olaylar Postgres `LISTEN/NOTIFY` ile örnekler arasında yayılır.

### 10.2 Bağlantı katmanı (istemci: `src/connectivity`)
Bağlantının koptuğu üç kaynaktan anlaşılır: işletim sistemi olayları (NetInfo / `navigator.onLine`), WS canlılık sinyali (25 saniye ping, 10 saniye zaman aşımı) ve başarısız HTTP istekleri. Durum makinesi:

```
BOOTING ──(bağlantı yok)──▶ OFFLINE_BLOCKED (cold start ekranı, "Tekrar dene")
BOOTING ──(ok)──▶ ONLINE ──(kopma)──▶ RECONNECTING (tam ekran kilit katmanı; artan aralıklarla yeniden deneme)
RECONNECTING ──(ok)──▶ ONLINE (+ tüm sorgular geçersiz kılınır ve tazelenir)
```

- **Yazma işlemleri (mutasyonlar):** Her mutasyon istemcide üretilen bir `idempotency_key` taşır, böylece aynı işlem iki kez kaydedilmez. İşlem başarısız olursa hata gösterilir ve form içeriği korunur. **Otomatik çevrimdışı kuyruk yoktur.**
- **Yjs:** Kopma anında yerel belgede kalan güncellemeler, yeniden bağlanınca y-sync ile kendiliğinden gönderilir (BP4). Kilit katmanı açık olduğu sürece editör salt okunurdur.

## 11. Work Session senkronu
- Timer durumu sunucuda tutulur ve sunucu bu konuda tek otoritedir: `state`, `started_at`, `accumulated_ms`, `planned_ms`.
- İstemci kalan süreyi kendisi hesaplar: `planned − (accumulated + (now_server − started_at))`. Saniye başı mesaj gönderilmez.
- Duraklat, devam et, durdur ve dilim değiştir komutları HTTP ile gönderilir. Sunucu durumu günceller ve `session.timer` olayını tüm katılımcılara yayınlar.
- Pomodoro süresinin bitişi hem istemcide (yerel bildirim) hem sunucuda (zamanlanmış iş + push bildirimi) ele alınır.

## 12. Arama
- `items` tablosundaki başlık ve açıklama alanlarından, ayrıca `page_snapshots.plain_text` alanından bir `tsvector` üretilir. Türkçe yapılandırma ve `unaccent` uzantısı kullanılır. Yazım hatalarına tolerans için `pg_trgm` eklenir.
- Her arama sorgusu `item_access` ile filtrelenir (AR2).
- Ölçek gerektirirse Meilisearch'e geçilebilir. Bunun için arama, `SearchIndex` arayüzünün arkasında tutulur.

## 13. Ek modül sistemi
```rust
struct ModuleManifest {
    key: &'static str,            // "subscriptions", "budget", "calories"...
    item_kinds: &'static [&'static str],
    migrations: &'static [Migration],
    routes: fn() -> Router,
    progress: Option<Box<dyn ProgressProvider>>,
    default_tracker_templates: &'static [TrackerTemplate],
}
```
- Modüller kullanıcı veya takım bazında açılıp kapatılır: `enabled_modules(owner, module_key)`.
- İstemcide modüller `features/modules/<ad>` klasöründe durur. Rota, ana sayfa widget'ı ve nesne kartı bileşenini bir kayıt (registry) üzerinden çekirdeğe bildirir.
- Abonelik takibi = tekrar kuralı (`recurrence`) + hatırlatma + bütçe takibine otomatik kayıt. Bütçe takibi = "sınırın altında kalmalı" yönünde bir takip.

## 14. Bildirimler ve arka plan işleri
- `jobs(id, kind, payload, run_at, attempts, locked_until, last_error)` tablosu, `FOR UPDATE SKIP LOCKED` ile çalışan işçiler tarafından tüketilir.
- Zamanlanmış işler:
  - rutin hatırlatmaları (kullanıcının saat dilimine göre),
  - unutulanlar taraması (günlük),
  - haftalık özet,
  - sayfa sıkıştırma,
  - hesap silme (30 gün sonra),
  - pomodoro bitişi.
- Push bildirimleri: mobilde Expo Push, masaüstünde Tauri bildirim eklentisi, web'de Web Push. E-posta için bir işlemsel e-posta sağlayıcısı kullanılır (sağlayıcı daha sonra seçilecek).

## 15. Kimlik doğrulama ve güvenlik
- Şifreler argon2id ile hash'lenir. E-posta doğrulaması ve şifre sıfırlama vardır.
- Oturum token'ı 256 bit rastgele bir değerdir; veritabanında SHA-256 hash'i saklanır.
  - Web: `HttpOnly`, `Secure`, `SameSite=Lax` cookie. Cookie kullanıldığı için CSRF token gerekir.
  - Mobil ve masaüstü: `Authorization: Bearer`. Token cihazda güvenli depoda saklanır.
- Hız sınırları: giriş, arkadaşlık isteği, dürtme ve davet uç noktalarında (`tower-governor`).
- Tüm yetki kontrolleri `PermissionService` üzerinden yapılır. Her uç nokta için hem yetkili hem yetkisiz senaryo test edilir.
- Kişisel veriler loglara yazılmaz.

## 16. Test stratejisi
| Seviye | Ne test edilir | Araç |
|---|---|---|
| Saf mantık | erişim hesabı, tekrar üretimi, ilerleme, tempo, streak, geriye dönük işaretleme sınırı | `cargo test` + `proptest` |
| Entegrasyon | API uç noktaları + gerçek Postgres; özellikle V1–V12 kuralları | testcontainers |
| İstemci | hook'lar, bileşenler, bağlantı durum makinesi | Vitest + React Native Testing Library |
| E2E | kritik akışlar (kayıt, görev oluşturma, paylaşım ve uyarı, birlikte düzenleme) | Playwright (web) |

Kapsam beklentisi: `access`, `tracking`, `recurrence` ve `goals` crate'lerinde dal kapsamı (branch coverage) ≥ %90.

## 17. Ortamlar ve dağıtım
- **local:** `docker compose up` ile Postgres çalışır, `cargo run` ile sunucu ve `pnpm dev` ile istemci (web) başlar.
- **staging/beta:** Tek VPS üzerinde Docker Compose: `server`, `postgres`, `caddy`. Web istemcisi Caddy tarafından statik olarak sunulur. Günlük `pg_dump` ve WAL arşivi S3 uyumlu depoya alınır.
- **CI (GitHub Actions):**
  - Rust: `fmt`, `clippy -D warnings`, `test`.
  - TS: `lint`, `typecheck`, `test`.
  - `api-types` güncel mi kontrolü: üretilen dosyalarda fark varsa CI başarısız olur.
  - Web build ve Playwright testleri.
- **Release:**
  - Masaüstü: Tauri build (GitHub Actions matrisi: macOS, Windows, Linux).
  - Mobil: EAS Build (`.ipa`, `.apk`).

## 18. Bilinen riskler
| Risk | Etki | Önlem |
|---|---|---|
| react-native-web'de sürükle-bırak (kanban) | Kullanıcı deneyimi | Spike T0.6.1 tamamlandı: Gesture Handler + Reanimated üzerinde ince katman (ADR-0018). |
| Mobilde WebView içinde çalışan editörün performansı | Kullanıcı deneyimi | Spike T0.6.2: 10tap + soket aktarımı ile web ↔ iOS ortak düzenleme çalıştı (ADR-0019). Gerçek cihazda gecikme ölçümü ve mobil araç çubuğu E2.5'te. |
| Yjs belge şemasının evrimi | Veri | Şema sürümleme ve dönüştürme testleri. |
| Takımlarda büyük ölçekte erişim yeniden hesaplama | Performans | Asenkron genişleme (§6.4); ölçek gerektirirse Zanzibar'a geçiş. |
| Rust'ta hazır paket eksikliği (kimlik doğrulama, iş kuyruğu) | Geliştirme hızı | Kapsamı dar tutulmuş kendi implementasyonlarımız. Gerekirse harici kimlik doğrulama servisi. |
| Mobilde mağaza içi satın alma zorunluluğu | Gelir | Monetizasyon fazında RevenueCat değerlendirilecek. |
