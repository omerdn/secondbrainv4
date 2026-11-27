# Roadmap

> Sürüm: 1.0 · Tarih: 2026-11-21
> Kaynaklar: [PRD.md](PRD.md) · [ARCHITECTURE.md](ARCHITECTURE.md) · [DECISIONS.md](DECISIONS.md)
> Görev takibi GitHub Project üzerinde yapılır: https://github.com/users/omerdn/projects/4

---

## GitHub'a aktarım kuralları

Bu doküman GitHub'a şu şekilde aktarılır:

| Roadmap öğesi | GitHub karşılığı |
|---|---|
| **Faz** (ör. `F0`) | **Milestone** (ör. `F0 · Temeller`) |
| **Epic** (ör. `E0.1`) | Issue, `epic` etiketiyle. Altındaki görevler bu issue'nun **sub-issue**'ları olur. |
| **Görev** (ör. `T0.1.1`) | Issue, ilgili epic'in sub-issue'su |

- **Issue başlığı:** `[T0.1.1] pnpm workspace + Turborepo kurulumu`. Köşeli parantez içindeki ID, issue'nun roadmap'teki yerini gösterir.
- **Etiketler:**
  - `faz:0` … `faz:7`
  - `alan:backend`, `alan:client`, `alan:infra`, `alan:docs`, `alan:design`
  - `tür:feature`, `tür:chore`, `tür:spike`, `tür:test`
  - `boyut:S` (≤ yarım gün), `boyut:M` (1–2 gün), `boyut:L` (3–5 gün)
  - `epic`
- **Issue gövdesi** şu bölümlerden oluşur: Bağlam (hangi PRD ve ARCHITECTURE maddeleri), Kapsam, Kabul kriterleri (madde listesi), Bağımlılıklar (issue ID'leri), Notlar.
- **Project durum sütunları:** Görevler `Backlog`ta başlar. Sırası gelen görev `Todo`ya, üzerinde çalışılan `In Progress`e, PR'ı açılan `In Review`a, merge edilen `Done`a taşınır.
- **Sıra:** Fazlar sırayla ilerler. Bir faz içindeki epic'ler, belirtilen bağımlılıklar elverdiği sürece paralel yürüyebilir.

Görev satırı formatı: `ID · başlık · alan · boyut`. Altındaki girintili satırlar issue'nun kapsamı ve kabul kriterleridir.

---

## F0 · Temeller

**Amaç:** Geliştirme altyapısını kurmak, kimlik doğrulama ve uygulama kabuğunu staging'de çalışır hale getirmek ve riskli teknik konuları baştan denemek.

**Çıkış kriteri:**
- Kullanıcı staging ortamında kayıt olup giriş yapabiliyor.
- Boş uygulama kabuğu web'de çalışıyor.
- Bağlantı politikası (BP1–BP5) çalışıyor.
- CI yeşil.
- Teknik keşiflerin (spike) sonuçları ADR olarak yazıldı.

### E0.1 · Monorepo ve araçlar
- **T0.1.1** · pnpm workspace + Turborepo kurulumu · infra · S
  - `apps/`, `packages/`, `server/` iskeleti
  - `packages/config` (eslint, tsconfig, prettier)
  - `pnpm dev`, `pnpm lint`, `pnpm typecheck`, `pnpm test` komutları çalışıyor
- **T0.1.2** · Cargo workspace iskeleti · backend · S
  - ARCHITECTURE §3'teki crate'ler boş olarak oluşturuldu
  - rustfmt/clippy ayarlarıyla `cargo build` ve `cargo clippy -D warnings` temiz geçiyor
- **T0.1.3** · Yerel geliştirme ortamı · infra · S
  - Docker Compose ile Postgres 17 (`pg_trgm`, `unaccent` uzantılarıyla)
  - `.env.example`
  - README'de kurulum adımları
- **T0.1.4** · GitHub Actions CI · infra · M
  - Rust: fmt, clippy, test (testcontainers ile)
  - TS: lint, typecheck, test
  - Önbellekleme
  - PR'larda zorunlu kontroller
- **T0.1.5** · PR ve issue şablonları, commit konvansiyonu · docs · S
  - Conventional Commits
  - PR şablonu: bağlı issue, test notları ve kontrol listesi
- **T0.1.6** · CLAUDE.md'ye oturumdan bağımsız iş akışı ayrıntıları · docs · S (#58)

### E0.2 · Sunucu iskeleti · bağımlılık: T0.1.2, T0.1.3
- **T0.2.1** · `app` crate · backend · S
  - axum, yapılandırma
  - graceful shutdown
  - `GET /health`
- **T0.2.2** · `kernel` crate · backend · M
  - UUIDv7 tipli Id'ler
  - hata tipi ve HTTP eşlemesi
  - `Clock` trait
  - sayfalama
  - doğrulama yardımcıları
- **T0.2.3** · `db` crate · backend · M
  - sqlx bağlantı havuzu
  - migration akışı
  - testcontainers tabanlı test yardımcısı (her test için izole bir şema)
- **T0.2.4** · Gözlemlenebilirlik · backend · S
  - `tracing`, request id
  - yapılandırılmış JSON log
  - Sentry entegrasyonu
  - Loglarda kişisel veri olmaması
- **T0.2.5** · Outbox ve domain olay dağıtıcısı · backend · M
  - Olay, aynı transaction içinde `outbox` tablosuna yazılır
  - Dağıtıcı olayları abonelere iletir
  - En az bir kez teslim edilir
  - Testli
- **T0.2.6** · `jobs` crate · backend · M
  - `SKIP LOCKED` ile çalışan işçi
  - Yeniden deneme ve artan bekleme süresi (backoff)
  - Zamanlanmış (cron) iş tanımı
  - Testli
- **T0.2.7** · ts-rs tip üretimi · backend · S
  - `packages/api-types` paketine üretim
  - Üretilen tiplerde fark varsa CI başarısız olur
- **T0.2.8** · Sunucu tarafı idempotency key işleme · backend · S
  - `Idempotency-Key` başlığı; ilk yanıt saklanır, tekrarında aynı yanıt döner
  - Eski anahtarlar periyodik olarak temizlenir

### E0.3 · Kimlik doğrulama (`identity`) · bağımlılık: E0.2
- **T0.3.1** · `users`, `auth_identities`, `sessions` migration'ları · backend · S
- **T0.3.2** · E-posta + şifre ile kayıt, giriş, çıkış · backend · M
  - argon2id
  - Opak token, veritabanında hash'li saklanır
  - Web'de cookie, mobil ve masaüstünde Bearer
  - Oturum listesi ve oturum kapatma
- **T0.3.3** · E-posta doğrulama ve şifre sıfırlama · backend · M
  - `EmailSender` arayüzü
  - Yerel ortam için log'a yazan sürücü
- **T0.3.4** · Google OAuth · backend · M
- **T0.3.5** · Apple ile giriş · backend · M
- **T0.3.6** · Hız sınırlama ve CSRF koruması · backend · S
- **T0.3.7** · Profil API · backend · S
  - Görünen ad, avatar URL, saat dilimi, dil
- **T0.3.8** · Süresi dolmuş oturumların temizlenmesi · backend · S

### E0.4 · İstemci iskeleti · bağımlılık: T0.1.1
- **T0.4.1** · Expo + Expo Router + react-native-web · client · M
  - Web'de çalışıyor
  - iOS simülatöründe açılıyor
- **T0.4.2** · NativeWind ve `packages/ui` token'ları · design · M
  - Renk, tipografi ve boşluk token'ları
  - Açık ve koyu tema
  - Takım rengi değişkeni
- **T0.4.3** · Temel bileşenler · client · L
  - Button, Input, Text, Card, Sheet/Modal, Toast, Avatar, TagChip, EmptyState
  - Hepsi web ve mobilde çalışıyor
- **T0.4.4** · `packages/api-client` · client · M
  - Tipli fetch, hata eşleme
  - İstemci tarafında idempotency key üretimi
  - TanStack Query kurulumu
- **T0.4.5** · Kimlik doğrulama ekranları · client · M
  - Kayıt, giriş, şifre sıfırlama
  - Google ve Apple ile giriş butonları
- **T0.4.6** · Uygulama kabuğu · client · M
  - Kenar çubuğu (web ve masaüstü), alt sekme çubuğu (mobil)
  - Responsive kırılım noktaları
- **T0.4.7** · i18n altyapısı · client · S
  - TR varsayılan, EN ikinci dil
- **T0.4.8** · `features/*` arasındaki import kısıtı · client · S
  - ESLint kuralı (ARCHITECTURE §4.6)
- **T0.4.9** · Sunucuda CORS (web istemcisi için) · backend · S (#74)
- **T0.4.10** · E-posta doğrulama ekranı · client · S (#77)
- **T0.4.11** · Android'de Google ile giriş · client · M (#78)
  - `zamanlama:en-son` (ADR-0024)

### E0.5 · Gerçek zamanlı kanal ve bağlantı politikası · bağımlılık: E0.3, T0.4.4
- **T0.5.1** · `realtime` crate · backend · M
  - `/ws` uç noktası, bağlantıda kimlik doğrulama
  - Kanal abonelikleri
  - ping/pong
  - Bağlantı kurulurken sunucu saatinin gönderilmesi
- **T0.5.2** · Postgres `LISTEN/NOTIFY` köprüsü · backend · S
  - Birden fazla sunucu örneği arasında olay yayını
- **T0.5.3** · İstemci bağlantı durum makinesi · client · M
  - ARCHITECTURE §10.2'deki durumlar
  - Birim testleri
- **T0.5.4** · Bağlantı ekranları · client · S
  - Cold start "Bağlantı yok" ekranı
  - Kopma anında tam ekran kilit katmanı
  - Form içeriğinin korunması (BP1–BP5)
- **T0.5.5** · WS olaylarıyla sorgu geçersiz kılma altyapısı · client · S
- **T0.5.6** · Oturum bitince WebSocket bağlantılarının kapatılması · backend · S (#85)
- **T0.5.7** · İstemcinin 4003 (oturum bitti) kapanışına tepki vermesi · client · S (#90)
- **T0.5.8** · Boş `EXPO_PUBLIC_API_URL` değerinin varsayılana düşmemesi · client · S (#93)

### E0.6 · Teknik keşifler (spike)
Her spike'ın sonucu bir ADR olarak yazılır.
- **T0.6.1** · Spike: react-native-web üzerinde sürükle-bırak kanban · client · M
  - Web ve iOS'ta çalışan bir prototip
  - Kütüphane kararı verilir
- **T0.6.2** · Spike: TipTap + Yjs ↔ yrs y-sync uçtan uca · backend · L
  - İki tarayıcı aynı sayfayı düzenleyebiliyor
  - Mobilde 10tap-editor denemesi
- **T0.6.3** · Spike: Tauri 2 ile web build paketleme · infra · S
  - macOS'ta açılıyor
  - Cookie ve WebSocket çalışıyor
- **T0.6.4** · CORS ve `/ws` için Tauri masaüstü origin'lerine izin · backend · S (#97)

### E0.7 · Staging ortamı · bağımlılık: T0.2.1
- **T0.7.1** · Sunucu Dockerfile ve Compose · infra · M
  - `server`, `postgres`, `caddy` servisleri
- **T0.7.2** · VPS kurulumu ve otomatik deploy · infra · M
  - TLS
  - `main` dalına merge edilen kod staging'e deploy edilir
- **T0.7.3** · Yedekleme · infra · S
  - `pg_dump` ve WAL arşivi S3 uyumlu depoya
  - Geri yükleme bir kez denenip dokümante edilir
- **T0.7.4** · Kullanıcıya görünen uygulama adının Routune olması · client · S (#102)
- **T0.7.5** · Beta paketleri öncesi mevcut ekranların UX düzeltmeleri · client · S (#104)
- **T0.7.6** · Routune ikonu, açılış ekranı ve favicon · design · S (#106)
- **T0.7.7** · Resend e-posta sürücüsü · backend · S (#108)
- **T0.7.8** · pnpm altında Android release JS paketinin düzeltilmesi · client · S (#110)

---

## F1 · Çekirdek domain

**Amaç:** Tüm özelliklerin üzerine kurulacağı yetki, nesne, takip ve tekrar motorlarını testli şekilde hazırlamak.

**Çıkış kriteri:**
- `access`, `tracking` ve `recurrence` crate'lerinde dal kapsamı ≥ %90.
- İstemci nesne kartlarını, etiketleri ve filtreleri gösterebiliyor.

### E1.1 · Yetki motoru (`access`) · bağımlılık: F0
- **T1.1.1** · `access_relations` ve `item_access` şeması · backend · S
- **T1.1.2** · `compute_access` saf fonksiyonu · backend · L
  - ARCHITECTURE §6.3'teki kurallar
  - proptest ile özellik testleri
  - Mirasla gelen erişimin asla MANAGE/DELETE vermediği test edilir
- **T1.1.3** · Etkilenen küme hesabı ve senkron yeniden hesaplama · backend · M
- **T1.1.4** · Büyük kümeler için asenkron genişleme · backend · M
  - Erişim kaldırma senkron, genişleme iş kuyruğunda
- **T1.1.5** · `PermissionService` · backend · M
  - `check`, `list_accessible`, `explain`, `preview` (V7, V11)
- **T1.1.6** · `AccessRevoked` olayı · backend · S
  - Erişimi düşen kullanıcının realtime bağlantıları kesilir (V10)
- **T1.1.7** · V1–V12 entegrasyon test paketi iskeleti · test · M
  - Her kural için bir test dosyası
  - Henüz implemente edilmemiş testler `#[ignore]` ile işaretlenir

### E1.2 · Nesne tabanı (`items`) · bağımlılık: E1.1
- **T1.2.1** · `items` şeması ve CRUD servisi · backend · M
  - Arşiv
  - 30 günlük çöp kutusu (N4)
- **T1.2.2** · Kişisel etiketler · backend · M
  - Oluşturma, düzenleme, nesneye takma
  - Görünürlük kuralı: kişisel etiketi sadece sahibi görür (E1, E2, E4)
- **T1.2.3** · Bağlantılar · backend · M
  - link / unlink
  - Gizli bağlantı filtresi (B1–B3, V4, V9)
- **T1.2.4** · Davet sistemi · backend · M
  - `invitations` tablosu, kabul ve red
  - Bildirim kancası (G5)
- **T1.2.5** · Aktivite kaydı · backend · S
  - A1, A2
- **T1.2.6** · `last_touched_at` güncelleme kuralları · backend · S
  - H4 için gerekli
- **T1.2.7** · Filtre motoru ve kayıtlı görünümler API · backend · M
  - F1, F2
- **T1.2.8** · Sahibin erişimi düşen sayfa bağlantılarının kaldırılması · backend · S
  - V13
- **T1.2.9** · Davet uç noktasında hız sınırı · backend · S
  - ARCHITECTURE hız sınırları; E3.1'de V1 açılmadan önce bitmeli

### E1.3 · Takip motoru (`tracking`) · bağımlılık: E1.2
- **T1.3.1** · `trackers` ve `tracker_entries` şeması · backend · S
- **T1.3.2** · Toplama yöntemleri ve dönem hesapları · backend · M
  - Saf fonksiyonlar
  - Testli
- **T1.3.3** · `ProgressProvider` trait'i ve takip implementasyonu · backend · M
  - "Artmalı" ve "sınırın altında kalmalı" yönleri (ARCHITECTURE §7)
- **T1.3.4** · Kayıt API · backend · S
  - Ekle, sil, listele
  - Idempotent
- **T1.3.5** · Takip tanımı API · backend · S
  - Oluştur, oku, düzenle (T1, K1); T2.4.1 için gerekli

### E1.4 · Tekrar motoru (`recurrence`) · bağımlılık: T0.2.2
- **T1.4.1** · Tekrar üretimi · backend · L
  - RRULE + plan sürümleri + saat dilimi
  - Saf fonksiyon
  - proptest ile: "ayın 31'i", "ayın son günü", yaz saati geçişleri
- **T1.4.2** · Rotasyon hesabı ve takaslar · backend · S

### E1.5 · İstemci çekirdeği · bağımlılık: E1.2, E0.4
- **T1.5.1** · Ortak nesne kartı ve nesne detay iskeleti · client · M
- **T1.5.2** · Etiket seçici ve etiketler ekranı · client · M
- **T1.5.3** · Filtre çubuğu · client · M
  - Kayıtlı görünümler kenar çubuğunda sabitlenebiliyor
- **T1.5.4** · Paylaşım önizleme onay diyaloğu · client · S
  - V7
- **T1.5.5** · "Bunu neden görüyorum?" paneli · client · S
  - V11
- **T1.5.6** · Çöp kutusu ve arşiv ekranları · client · S
- **T1.5.7** · Filtre çubuğunda tarih, durum ve kişi · client · S (#207)
  - Veri kaynakları sonraki fazlarda geldi (görev durumları F2, arkadaşlar F3); F3 içinde yapılır. Takım filtresi T5.1.7'ye taşındı.
- **T1.5.8** · Sheet içeriğinin klavyenin üstünde kalması · client · S

---

## F2 · Kişisel MVP

**Amaç:** Tek bir kullanıcının tüm çekirdek özellikleri web'de kullanabilmesi. Bu fazın sonunda ekip uygulamayı kendisi kullanmaya başlar (dogfooding).

**Çıkış kriteri:**
- Görev, rutin, hedef, takip, defter, Work Session, Bugün ekranı, takvim, arama ve bildirim merkezi uçtan uca çalışıyor.
- Kritik akışların Playwright testleri var.
- Her epic sonunda web'de kısa doğrulama yapıldı (ADR-0024).

### E2.1 · Görevler · bağımlılık: F1
- **T2.1.1** · Görev, alt kırılım ve atanan kişiler: şema ve API · backend · M
  - G1–G4
  - Görev alt kırılımsız oluşturulursa başlığıyla otomatik alt kırılım oluşur (G2)
- **T2.1.2** · Görev `ProgressProvider` · backend · S
  - G3
- **T2.1.3** · Kişisel kanban API · backend · M
  - Sütun oluşturma ve düzenleme, görev yerleşimi
  - Gelen kutusu sütunu
  - Kesirli indeksle sıralama (G6)
- **T2.1.4** · İstemci: görev oluşturma ve düzenleme · client · M
  - Alt kırılım listesi
  - Tarihler
  - Etiketler
- **T2.1.5** · İstemci: kanban görünümü · client · L
  - Sürükle-bırak (T0.6.1'in sonucuna göre)
  - Sütun özelleştirme
- **T2.1.6** · İstemci: görev liste görünümü · client · S
- **T2.1.7** · İstemci: kanban otomatik kaydırma ve uzun sütun sanallaştırması · client · M
  - ADR-0018'den kalan işler

### E2.2 · Rutinler · bağımlılık: F1
- **T2.2.1** · Rutin ve plan sürümleri: şema ve API · backend · M
  - R1–R3, R11–R13
- **T2.2.2** · Tamamlama · backend · M
  - Kayıt olarak tutulur, günlük hedef sayısı
  - Kısmi tamamlama sayılır (R4)
  - Geriye dönük en fazla 3 gün (R7)
- **T2.2.3** · Streak hesabı · backend · M
  - R9
  - Testli
- **T2.2.4** · Rutin `ProgressProvider` · backend · M
  - Hedef ve rutin aralıklarının kesişimi (D5)
  - Günlük katkı hedefle sınırlanır
- **T2.2.5** · İstemci: rutin oluşturma · client · L
  - Tekrar seçici (günlük, haftalık, aylık, yıllık, N'de bir, ayın son günü)
  - Günlük hedef sayısı
  - Hatırlatma saatleri
- **T2.2.6** · İstemci: rutin işaretleme · client · M
  - +1 sayaç
  - Isı haritası (R8)
  - Streak gösterimi
- **T2.2.7** · Rutin hatırlatma işi · backend · S
  - Şimdilik uygulama içi bildirim. Push, F4'te eklenir.

### E2.3 · Hedefler · bağımlılık: E2.1, E2.2
- **T2.3.1** · Hedef ve hedef bağlantıları API · backend · S
  - Ağırlıklar (D1, D2)
- **T2.3.2** · Hedef hesabı · backend · M
  - Gerçekleşen (R), beklenen (E), tempo (D3–D6)
  - Her nesnenin katkı dökümü
  - proptest
- **T2.3.3** · İstemci: hedef oluşturma · client · M
  - Nesne bağlama
  - Ağırlık düzenleme
- **T2.3.4** · İstemci: hedef detayı · client · M
  - Gerçekleşen ve tempo göstergeleri
  - Katkı dökümü

### E2.4 · Takipler · bağımlılık: E1.3
- **T2.4.1** · İstemci: genel takip · client · M
  - Oluşturma: birim, hedef değer, dönem, yön
  - Kayıt girme (isteğe bağlı notla)
  - Geçmiş grafiği (K1–K3)
- **T2.4.2** · Takip dönem özeti API · backend · S
  - Dönem içi kullanım, kalan ve sınır aşımı (K2)
  - T2.4.1 buna bağlıdır

### E2.5 · Defterler ve sayfalar · bağımlılık: F1, T0.6.2
- **T2.5.1** · Defter ve sayfa şeması · backend · S
  - Kullanıcının varsayılan defterinin otomatik oluşturulması
- **T2.5.2** · Collab sunucusu · backend · L
  - `/collab/:page` uç noktası
  - y-sync protokolü
  - Bağlantıda yetki kontrolü (EDIT yoksa salt okunur)
  - Güncellemelerin kalıcı olarak saklanması
- **T2.5.3** · Snapshot ve sıkıştırma işi · backend · M
  - Arama için `plain_text` çıkarımı
- **T2.5.4** · Sürüm geçmişi API ve eski sürüme dönüş · backend · M
- **T2.5.5** · Süreç sayfası · backend · M
  - Otomatik oluşturma
  - Sunucunun sayfaya tarihli satır eklemesi
  - Kayıttaki `note_block_id` bağı (S1–S3, T3)
- **T2.5.6** · İstemci: defter listesi ve sayfa ağacı · client · M
- **T2.5.7** · İstemci: TipTap editörü (web) · client · L
  - Yjs sağlayıcısı
  - Araç çubuğu (Y3, Y4)
- **T2.5.8** · Embed'ler ve link önizlemesi · client · M
  - Sunucu tarafında oEmbed/meta proxy'si
  - SSRF koruması
- **T2.5.9** · İstemci: sayfa bağlama arayüzü · client · S
  - Bağlamadan önce önizleme diyaloğu
- **T2.5.10** · İstemci: son durum satırı · client · S
  - Nesne kartlarında ve detayında (S3)

### E2.6 · Work Session (kişisel) · bağımlılık: E1.2
- **T2.6.1** · Şema ve timer durum makinesi API · backend · M
  - W1–W3
- **T2.6.2** · Dilim log'u ve istatistik API · backend · M
  - W4, W5
- **T2.6.3** · İstemci: timer ekranı · client · L
  - Pomodoro, kronometre ve mola modları
  - Duraklatınca bağlı nesneyi değiştirme
  - Dilim log'u
- **T2.6.4** · İstemci: geçmiş ve istatistik ekranı · client · M

### E2.7 · Ana sayfa, takvim ve arama · bağımlılık: E2.1, E2.2, E2.3
- **T2.7.1** · "Bugün" API ve unutulanlar sorgusu · backend · M
  - H1–H6
- **T2.7.2** · İstemci: Bugün ekranı · client · M
- **T2.7.3** · Takvim API · backend · M
  - Görev tarihleri, rutin tekrarları, hedef aralıkları
- **T2.7.4** · İstemci: takvim görünümleri · client · L
  - Gün, hafta ve ay
- **T2.7.5** · Arama · backend · M
  - `tsvector`, Türkçe yapılandırma, trigram
  - `item_access` filtresi (AR1, AR2)
- **T2.7.6** · İstemci: global arama · client · M
  - Klavye kısayolu
  - Sonuç listesi
- **T2.7.7** · E2E: kalan kritik akışlar için Playwright testleri · client · M
  - Görev ve kanban, hedef ilerlemesi, takip kaydı, çalışma oturumu, arama, takvim
  - F2 çıkış kontrolünde eklendi

### E2.8 · Bildirim merkezi (uygulama içi) · bağımlılık: E1.2
- **T2.8.1** · Bildirim şeması, API ve tercihler · backend · M
  - BL1, BL3
- **T2.8.2** · İstemci: bildirim merkezi · client · S

### Faz içi düzenlemeler (sonradan açılan)
- **T2.0.1** · Tekrarlayan oturum kurallarının CLAUDE.md'ye taşınması · docs · S (#315)

---

## F3 · Sosyal ve ortak kullanım

**Amaç:** Arkadaşlarla ortak görev, rutin, defter ve çalışma oturumu kullanılabilmesi. Tüm görünürlük kurallarının testle güvence altına alınması.

**Çıkış kriteri:**
- Takımlara bağlı olmayan V1–V12 senaryoları aktif ve yeşil. Takım senaryoları (`T5.1.x`) F5'te açılır.
- İki arkadaş ortak tüm akışları kullanabiliyor.

### E3.1 · Arkadaşlık · bağımlılık: F2
- **T3.1.1** · Arkadaşlık API · backend · M
  - İstek gönderme, kabul, red, arkadaşlıktan çıkarma, engelleme
  - Hız sınırları ve tekrar istek bekleme süresi (AK1, AK2)
- **T3.1.2** · İstemci: arkadaşlar ekranı · client · M
  - İstek kutusu
  - Profil kartı

### E3.2 · Ortak nesneler · bağımlılık: E3.1
- **T3.2.1** · Nesneye üye davet etme ve çıkarma · backend · M
  - Sahip, sorumlu ve izleyici rolleri (N2, G5, V1, V5)
- **T3.2.2** · Miras görünürlüğün uçtan uca çalışması · backend · M
  - V2–V4, V8
  - Önizleme uyarıları (V7)
- **T3.2.3** · Alt kırılım atama · backend · S
  - G4
- **T3.2.4** · Arkadaş filtresi · backend · S
  - F3
- **T3.2.5** · İstemci: üye yönetimi · client · M
  - Davet bildirimleri
  - Kabul ve red
- **T3.2.6** · Takım dışı görünürlük testlerinin tamamlanması · test · M
  - `T3.x` etiketli `#[ignore]` testlerin tamamı aktif; takımlara bağlı `T5.1.x` senaryoları F5'e kalır

### E3.3 · Ortak rutinler · bağımlılık: E3.2
- **T3.3.1** · Tamamlama modları · backend · M
  - Ortak havuz ve herkes ayrı (R5)
  - Ortak streak (R10)
- **T3.3.2** · Nöbet/rotasyon ve takas · backend · M
  - R6
- **T3.3.3** · İstemci: ortak rutin görünümü · client · M
  - Kim, ne zaman yaptı
  - Rotasyon arayüzü
- **T3.3.4** · Ortak rutin hatırlatmalarının sorumlulara gitmesi · backend · S (#348)
  - Rotasyonda o tekrarın sorumlusu; sabit sorumlularda hedefine ulaşmamış kişiler (R13, R6, R5)

### E3.4 · Ortak defterler · bağımlılık: E3.2
- **T3.4.1** · Defter üyeliği API · backend · S
  - editor ve viewer rolleri (Y2)
- **T3.4.2** · Canlı imleçler ve farkındalık (awareness) · client · S
  - Y3
- **T3.4.3** · İstemci: defter paylaşım arayüzü · client · S
  - "Ortak" rozeti

### E3.5 · Sosyal etkileşimler · bağımlılık: E3.1
- **T3.5.1** · Dürtme · backend · S
  - Aynı kişiye günde en fazla 1 (AK3)
  - Engellenen kişi dürtemez
- **T3.5.2** · Emoji tepkileri · backend · S
  - AK4
- **T3.5.3** · İstemci: dürtme, tepkiler ve yan yana ilerleme görünümü · client · M
  - AK5

### E3.6 · Ortak Work Session · bağımlılık: E2.6, E3.1
- **T3.6.1** · Ortak oturum altyapısı · backend · M
  - Katılımcılar
  - Ortak timer olayları
  - Kişisel dilimler (W6)
- **T3.6.2** · İstemci: ortak oturum · client · M
  - Davet etme ve katılma
  - Canlı timer

### E3.7 · E-posta · bağımlılık: T0.3.3
- **T3.7.1** · E-posta sağlayıcısı entegrasyonu · backend · M
  - Davet, doğrulama ve şifre sıfırlama şablonları

### Faz içi düzenlemeler (sonradan açılan)
- **T3.0.1** · Takım görünürlük testlerinin F5 çıkış kriterlerine taşınması · docs · S (#289)
- **T3.0.2** · ROADMAP'in açılmış issue'larla eşitlenmesi ve T1.5.7'nin bölünmesi · docs · S (#349)
- **T3.0.3** · Görev durumunun öğeye yazılması (açık, tamamlandı) · backend · S (#351)
  - F1 durum filtresi için; T1.5.7'nin bağımlılığı

---

## F4 · Masaüstü, mobil ve kapalı beta

**Amaç:** Uygulamanın tüm platformlarda çalışması ve eş-dost kapalı betasının başlaması.

**Çıkış kriteri:**
- `.dmg`, `.exe`, `.apk` ve `.ipa` dosyaları üretiliyor.
- Push bildirimleri çalışıyor.
- Hesap silme ve veri dışa aktarma hazır.
- Beta kullanıcıları uygulamayı kullanıyor.

> **Not (ADR-0023, 2026-11-22):** E4.1'in dağıtım ve güncelleme görevleri ile E4.2'deki T4.2.5–T4.2.7, sahibin kararıyla F1'den önce yapılır.

> **Not (ADR-0024, 2026-11-22):** Kalan platform işleri (T4.1.2, T4.2.1–T4.2.3) ve beta paketlerinin çıkarılması tüm fazlar bittikten sonra yapılır. Ücretli Apple Developer hesabı gerektiren iş yapılmaz; iOS push kapsam dışıdır. T4.2.3 sadece Android'de, FCM ile yapılır. T4.3.3 yasal onay altyapısına dönüştü; metinler T4.3.6'ya taşındı ve en sona kaldı. T4.3.4 (uygulama içi geri bildirim) en sona kaldı; beta geri bildirimleri GitHub issue'larıyla toplanır. T4.4.2 ADR-0021 gereği yapılmaz.

### E4.1 · Masaüstü · bağımlılık: T0.6.3
- **T4.1.1** · Tauri 2 uygulaması · infra · M
  - Web build'inin paketlenmesi
  - Pencere yönetimi
  - Bearer oturum (güvenli depo) ve Rust WebSocket (ADR-0020)
- **T4.1.2** · Masaüstü bildirimleri · client · S
- **T4.1.3** · Build matrisi · infra · M
  - GitHub Actions ile `.dmg`, `.exe` ve AppImage, GitHub Releases
- **T4.1.4** · Masaüstü otomatik güncelleme · client · M
  - `tauri-plugin-updater`, imzalı güncellemeler (ADR-0023)
- **T4.1.5** · Masaüstü deep link · client · S
  - T4.1.1'den ayrıldı

### E4.2 · Mobil
- **T4.2.1** · iOS ve Android uyumluluk turu · client · L
  - Navigasyon, klavye davranışı, güvenli alanlar, jestler
- **T4.2.2** · Mobil editör · client · M
  - 10tap-editor entegrasyonu
- **T4.2.3** · Android push bildirimleri (FCM) · backend · M
  - Cihaz token yönetimi
- **T4.2.4** · Web Push · backend · S
- **T4.2.5** · Mobil paketler (CI) · infra · M
  - APK, iOS simülatör paketi ve imzasız `.ipa`; EAS Build yerine kendi CI'ımız (ADR-0023)
- **T4.2.6** · Mobil OTA güncellemeleri · client · M
  - `expo-updates`, kendi sunucumuzdan (ADR-0023)
- **T4.2.7** · Minimum desteklenen istemci sürümü · backend · S

### E4.3 · Beta öncesi zorunlular
- **T4.3.1** · Hesap silme akışı · backend · M
  - Sahiplik devri rehberi
  - 30 gün sonra otomatik devir ve silme işi (HS2)
- **T4.3.2** · Veri dışa aktarma · backend · M
  - JSON ve Markdown (HS3)
- **T4.3.3** · Yasal onay altyapısı · backend · M
  - Yer tutucu metinli sayfalar, kayıtta zorunlu onay, onay sürümünün kaydı (HS5)
- **T4.3.4** · Uygulama içi geri bildirim ve hata bildirimi · client · S
- **T4.3.5** · Haftalık özet · backend · M
  - Hesaplama işi
  - E-posta ve uygulama içi gösterim (OZ1)
- **T4.3.6** · Yasal metinler · docs · S
  - Gizlilik politikası, KVKK aydınlatma metni, kullanım koşulları (HS5); en sona

### E4.4 · Beta
- **T4.4.1** · Beta kurulum dokümanı · docs · S
  - Her platform için kurulum talimatları
  - İmzasız uygulama uyarılarının nasıl geçileceği
- **T4.4.2** · Production ortamının staging'den ayrılması · infra · M
- **T4.4.3** · Beta başlangıcı · docs · S
  - Geri bildirimleri sınıflandırma ve önceliklendirme süreci

### Faz içi düzenlemeler (sonradan açılan)
- **T4.0.1** · Canlı ortam ve sürüm düzeninin CLAUDE.md'de özetlenmesi · docs · S (#128)
- **T4.0.2** · Web öncelikli iş akışının yazılması ve platform sürümlerinin ertelenmesi · docs · S (#176)

---

## F5 · Takımlar

**Amaç:** Takımların rol ve yetkilerle birlikte görev, rutin, hedef ve defter kullanabilmesi.

**Çıkış kriteri:** Bir takım rol matrisiyle birlikte tüm çekirdek özellikleri kullanabiliyor. Büyük takım performans testi geçiyor. Kalan `T5.1.x` görünürlük senaryoları açıldı; V1–V12 paketinin tamamı aktif ve yeşil.

### E5.1 · Takım temeli · bağımlılık: F3
- **T5.1.1** · Takım ve üyelik · backend · M
  - `teams`, `team_members`
  - Katılım daveti, kabul ve red (TK1, TK2)
- **T5.1.2** · Roller ve yetki matrisi · backend · L
  - `PermissionService` entegrasyonu (TK3)
- **T5.1.3** · Takım nesneleri · backend · M
  - Sahip takım
  - Görünürlük modu: tüm takım veya sadece atananlar (V12)
  - Sorumlu ve izleyici rolleri (TK4)
- **T5.1.4** · Takım etiketleri · backend · M
  - E3
  - İstemcide takım rengi ve ayırt edici stil
- **T5.1.5** · Takım defterleri · backend · M
  - Y2
- **T5.1.6** · Takımdan ayrılma · backend · S
  - TK7
- **T5.1.7** · Filtre çubuğunda takım · client · S
  - T1.5.7'den ayrıldı; takım listesi ve kayıtlı görünümde saklama (F1, F3)

### E5.2 · Takım arayüzü · bağımlılık: E5.1
- **T5.2.1** · Takım ayarları sayfası · client · L
  - TK5
- **T5.2.2** · Takım panosu · client · L
  - TK6
- **T5.2.3** · Aktivite log görünümü ve filtreleri · client · M
- **T5.2.4** · Büyük takımlarda erişim hesabı performans testi · test · M
  - 1000 üye ve 50 bin nesne

---

## F6 · Modül sistemi ve modül paketleri

**Amaç:** Modül altyapısını kurmak ve ilk modül paketlerini yayınlamak.

**Çıkış kriteri:** Abonelik ve bütçe paketi kullanılabiliyor. Modüller kullanıcı veya takım bazında açılıp kapatılabiliyor.

### E6.1 · Modül altyapısı · bağımlılık: F2
- **T6.1.1** · `ModuleManifest`, modül kaydı, açma ve kapama · backend · M
  - M1
- **T6.1.2** · İstemci modül kaydı (registry) · client · M
  - Modülün rotası, ana sayfa widget'ı ve nesne kartı

### E6.2 · Abonelik ve bütçe paketi · bağımlılık: E6.1
- **T6.2.1** · Bütçe takibi · backend+client · M
  - Kategoriler
  - Aylık sınır
- **T6.2.2** · Abonelik takibi · backend+client · M
  - Yenileme tarihleri ve hatırlatma
  - Bütçeye otomatik kayıt

### E6.3 · Sonraki modüller · bağımlılık: E6.1
- **T6.3.1** · Dizi/film takibi · backend+client · L
  - Arkadaşla ortak ilerleme
- **T6.3.2** · Kitap ve kurs takibi · backend+client · M
- **T6.3.3** · Kalori takibi · backend+client · M
- **T6.3.4** · Eşya takibi · backend+client · M

---

## F7 · Monetizasyon, entegrasyonlar ve yayın

**Amaç:** Gelir modelini devreye almak, dış entegrasyonları eklemek ve uygulamayı mağazalarda yayınlamak.

> **Not (sahip kararı, 2026-11-27):** F7'nin monetizasyon, takvim, Wrapped, widget ve yayın işleri betada gerekmez. Bunlar v1 sürümünde, herkese açılırken yapılır; board'da "Going to be done in the release" sütununda ve `zamanlama:sürüm` etiketlidir. Ücretli paketlerin içeriği henüz belirlenmedi. F7'de şimdilik yalnız T7.3.3 (şablon galerisi ve mağazası altyapısı) yapılır.

### E7.1 · Monetizasyon
- **T7.1.1** · Paket tanımı ve yetkilendirme (entitlement) altyapısı · backend · M
- **T7.1.2** · Web ve masaüstü ödeme (Stripe) · backend · L
- **T7.1.3** · Mobil mağaza içi satın alma · backend+client · L
  - RevenueCat değerlendirmesi ve entegrasyonu (GM2)

### E7.2 · Takvim entegrasyonları
- **T7.2.1** · iCal feed · backend · S
- **T7.2.2** · Google Takvim ve Apple Takvim · backend · L

### E7.3 · Etkileşim
- **T7.3.1** · Yıl sonu Wrapped · backend+client · L
  - OZ2
- **T7.3.2** · Widget'lar · client · L
  - iOS ve Android ana ekran widget'ları (native kod gerektirir)
  - macOS menü çubuğu
- **T7.3.3** · Şablon galerisi ve mağazası altyapısı · backend+client · L
  - Şablon modeli, galeri, şablondan oluşturma; ödeme ve herkese açık yayımlama yok

### E7.4 · Yayın
- **T7.4.1** · Kod imzalama · infra · M
  - Apple notarization, Windows imzalama
- **T7.4.2** · App Store ve Play Store yayını · infra · M
- **T7.4.3** · Masaüstü otomatik güncelleme · infra · M
  - Tauri updater


### Faz içi düzenlemeler (sonradan açılan)
- **T7.0.1** · F7'nin sürüm işlerinin "release" sütununa alınması · docs · S (#405)
---

## Backlog (faza atanmamış)
- Notion'dan tek yönlü içe aktarma (Notion → uygulama)
- Doğal dille nesne oluşturma
- Kotalı dosya yükleme (ücretli)
- Todoist ve Habitica'dan içe aktarma
- SpiceDB'ye geçiş (sadece ölçek gerektirirse; ADR-0004)
- Yapay zekâ özellikleri ("Nerede kalmıştım?" özeti vb.; maliyet uygun olduğunda)
