# Mimari Karar Kayıtları (ADR)

> Her kararın kaydı burada tutulur. Bir karar değiştirilecekse eski kayıt silinmez: durumu **Geçersiz (yerine ADR-XXXX)** olarak işaretlenir ve yeni bir kayıt eklenir.
> Format: Bağlam → Karar → Sonuçlar → Değerlendirilen alternatifler.

---

## ADR-0001 · Tek workspace, etiket bazlı ayrım
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Kullanıcının kişisel, arkadaşla ortak ve takım nesneleri var. Bunları ayrı workspace'lere bölmek, kontrolü tek bir yerde toplama amacıyla çelişiyor.
- **Karar:** Tek bir workspace olacak. Ayrım şu araçlarla yapılacak:
  - kişisel ve takım etiketleri,
  - arkadaş ve takım filtreleri,
  - kayıtlı görünümler.
  
  Takım etiketleri ayrı listelerde değil, renk ve stil ile ayrışır.
- **Sonuçlar:** Görünürlük, workspace sınırlarına değil nesne bazlı yetki modeline dayanmak zorunda (ADR-0004). Takım ayarları bir workspace değil, bir ayarlar ekranı olacak.
- **Alternatifler:** Ayrı workspace'ler. Bağlamı böldüğü için reddedildi.

## ADR-0002 · Hibrit stack: Expo + react-native-web arayüz, Tauri masaüstü, Rust sunucu
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Hedef platformlar web, masaüstü, iOS ve Android. Her platform için ayrı kod yazılmak istenmiyor. Performans önemli ve Rust tercih ediliyor. Ancak saf Rust arayüz kütüphaneleri (Dioxus, Slint vb.) mobilde ve zengin metin editöründe henüz olgun değil.
- **Karar:**
  - Arayüz: Expo (React Native) + react-native-web ile tek kod tabanı.
  - Masaüstü: Web çıktısı Tauri 2 ile paketlenir.
  - Sunucu: Rust (axum, sqlx, PostgreSQL).
  - Tip paylaşımı: TypeScript tipleri Rust'tan `ts-rs` ile üretilir.
- **Sonuçlar:**
  - Rust, en çok değer kattığı yerde kullanılıyor: sunucu, yetki ve ilerleme hesapları, CRDT sunucusu ve masaüstü kabuğu.
  - Arayüz React ekosisteminin genişliğinden yararlanıyor.
  - Bedeli: Rust'ta bazı hazır paketler yok (kimlik doğrulama, iş kuyruğu), bunları kendimiz yazacağız.
- **Alternatifler:**
  - Saf Rust arayüz: olgunluk eksikliği nedeniyle reddedildi.
  - Flutter: TS/Rust ekosisteminin dışında kaldığı ve CRDT desteği zayıf olduğu için reddedildi.
  - Tamamen TypeScript (Node backend): Rust tercihi ve tip güvenliği nedeniyle reddedildi.
  - Electron: Tauri daha hafif olduğu için reddedildi.

## ADR-0003 · Sadece çevrimiçi
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Çevrimdışı düzenleme ve sonradan birleştirme, senkronizasyon çatışmaları ve yüksek geliştirme maliyeti getiriyor.
- **Karar:**
  - Bağlantı yoksa uygulama açılmaz.
  - Bağlantı koparsa uygulama kilitlenir.
  - Mutasyonlar idempotent yapılır.
  - Otomatik çevrimdışı kuyruk olmaz.
  - Bağlantı kontrolü saniye başı yapılmaz; işletim sistemi olayları, WS ping'i ve başarısız istekler kullanılır.
- **Sonuçlar:** Çevrimdışı senkronizasyonla ilgili sorunların (çatışma, erişimi kaldırılmış birinin çevrimdışı düzenlemesi vb.) tamamı ortadan kalkar. Yjs yine de kullanılır, ama amacı **eşzamanlı çevrimiçi düzenlemede** veri kaybını önlemek.
- **Alternatifler:**
  - Tam çevrimdışı çalışma (local-first): maliyetli olduğu için reddedildi.
  - Salt okunur önbellek: kullanıcının internet var sanıp yanılmasına yol açacağı için reddedildi.

## ADR-0004 · Yetki: Postgres'te Zanzibar tarzı ilişkiler + türetilmiş erişim önbelleği
**Durum:** Kabul · 2026-11-21

- **Bağlam:**
  - Görünürlük, birbirine zincirlenen ilişkilerle belirleniyor: sahip, üye, takım rolü, defter üyeliği, bağlantı yoluyla miras.
  - Listeleme ve arama sorgularının hızlı olması gerekiyor.
  - Ayrı bir yetki servisi (SpiceDB/OpenFGA) şu an gereksiz işletme yükü getirir.
- **Karar:**
  - Erişimi etkileyen ilişkiler `access_relations` tablosunda, tuple formatında tutulur. Bu tablo tek doğruluk kaynağıdır.
  - Etkin yetkiler `item_access` tablosunda önceden hesaplanmış olarak tutulur.
  - Erişim hesaplaması saf bir fonksiyondur.
  - Tüm yetki kontrolleri `PermissionService` arayüzünden geçer.
- **Sonuçlar:**
  - Listeleme sorguları basit bir JOIN ile yapılır.
  - "Bunu neden görüyorum?" ve önizleme özellikleri doğal olarak sağlanır.
  - Ölçek gerektirirse SpiceDB'ye geçiş birkaç haftalık iştir: tuple'lar kopyalanır ve arayüzün arkasındaki implementasyon değiştirilir.
- **Alternatifler:**
  - Şimdiden SpiceDB: erken bir karmaşıklık getirdiği için reddedildi.
  - Postgres RLS: zincirli miras kurallarını RLS ile yazmak zor olduğu için reddedildi.

## ADR-0005 · Defterlerde CRDT: Yjs (istemci) + yrs (sunucu)
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Birden fazla kişinin aynı sayfayı aynı anda düzenlemesi sırasında veri kaybı olmamalı.
- **Karar:**
  - Editör: TipTap + Yjs.
  - Sunucu: yrs ile y-sync protokolü.
  - Güncellemeler sadece ekleme şeklinde (append-only) saklanır, periyodik olarak snapshot alınır.
  - Süreç sayfasına sunucu tarafından satır eklenebilir.
- **Sonuçlar:**
  - Karakter düzeyinde birleştirme yapılır ve veri kaybı olmaz.
  - Sürüm geçmişi elde edilir.
  - Editör şemasının sürümlenmesi gerekir.
- **Alternatifler:**
  - Operational Transformation (OT): sunucu tarafı karmaşıklığı nedeniyle reddedildi.
  - Kilitleme veya "çakışan kopya" üretmek: kullanıcı deneyimini bozduğu için reddedildi.
  - Liveblocks gibi yönetilen servisler: maliyet ve bağımlılık nedeniyle reddedildi.

## ADR-0006 · Takip motoru çekirdekte
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Rutinler, görevler, hedefler ve ek modüllerin hepsi "sayılarla ilerleme" kavramına dayanıyor. Her biri için ayrı hesap mantığı yazmak spagetti koda yol açar.
- **Karar:**
  - Tek bir takip motoru olacak. Takip; birim, hedef değer, dönem, toplama yöntemi ve yön alanlarından oluşur. Takibe girilen değerler kayıt olarak tutulur.
  - İlerlemeye katkı veren her nesne türü `ProgressProvider` arayüzünü uygular.
  - Kayıt notları süreç sayfasında yaşar (tek kaynak).
  - Pazarlamada "tracker" kelimesi kullanılmaz, bunun yerine modüller sunulur.
- **Sonuçlar:** Yeni bir takip modülü, sadece bir takip şablonu ve bir arayüzden oluşur. Hedef hesabı tek bir yerde yaşar.
- **Alternatifler:** Her modülün kendi ilerleme hesabı: tekrarlı kod ve tutarsızlık nedeniyle reddedildi.

## ADR-0007 · Modüler monolit ve katı modül sınırları
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Ürün ERP ve SaaS gibi modüler büyüyecek. Ekip küçük ve mikroservislerin işletme maliyeti yüksek.
- **Karar:**
  - Tek bir sunucu süreci çalışır, her domain modülü ayrı bir crate'tir.
  - Bir modül başka bir modülün tablolarına erişemez.
  - Modüller arası iletişim, servis arayüzleri ve outbox tabanlı domain olaylarıyla yapılır.
  - İstemci tarafında da `features/*` klasörleri arasında import kısıtı vardır.
- **Sonuçlar:** Kod tabanı modül modül anlaşılabilir kalır. İleride bir modül gerekirse ayrı bir servise çıkarılabilir.
- **Alternatifler:** Mikroservisler (erken) ve sınırsız monolit (spagetti riski) reddedildi.

## ADR-0008 · Tekrar kuralları RRULE ile; tekrarlar saklanmaz
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Tekrar kuralları RFC 5545 RRULE formatında tutulur.
  - Plan değişiklikleri için kurallar sürümlenir (`routine_schedules`).
  - Tekrar tarihleri ihtiyaç anında hesaplanır; veritabanında sadece tamamlamalar (kayıtlar) saklanır.
  - Her rutinin kendi saat dilimi vardır.
- **Sonuçlar:**
  - Plan değiştiğinde geçmiş bozulmaz.
  - Veritabanında milyonlarca boş tekrar satırı birikmez.
  - Tekrar hesapları saf fonksiyonlar olduğu için kolayca test edilir.

## ADR-0009 · Redis yok: Postgres tabanlı iş kuyruğu ve LISTEN/NOTIFY
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Sistemde olabildiğince az sayıda servis olsun isteniyor ve şu anki ölçek küçük.
- **Karar:**
  - Arka plan işleri, `FOR UPDATE SKIP LOCKED` ile çalışan Postgres tabanlı bir iş tablosundan yürütülür.
  - Birden fazla sunucu örneği arasında olay yayını Postgres `LISTEN/NOTIFY` ile yapılır.
- **Sonuçlar:** İşletilecek tek veri servisi Postgres olur. Yük artarsa ileride Redis veya NATS eklenebilir.

## ADR-0010 · Hedef ilerlemesi: ağırlıklı ortalama, "gerçekleşen + tempo" birlikte
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - `R = Σw·r / Σw` ve `Tempo = R / E` (formüller ARCHITECTURE §7).
  - Rutinler hedefe sadece tarih kesişimi içindeki tekrarlarla katkı verir.
  - Günlük katkı hedefle sınırlanır (hedefin üstüne çıkmak başka günü kapatmaz). Kısmi günler sayılır.
  - Streak sadece tam tamamlanan tekrarlarla ilerler.
- **Sonuçlar:** Hedef ilerlemesi doğru, açıklanabilir ve motive edici olur. Tempo göstergesi, henüz başlamamış rutinlerin hedefi düşük gösterip kullanıcının moralini bozmasını engeller.

## ADR-0011 · Defter → Sayfa; iki sahiplik türü
**Durum:** Kabul · 2026-11-21

- **Bağlam:** Notların dağınıklığı ve paylaşımla ilgili mahremiyet sorunları.
- **Karar:**
  - Notlar "Defterler" olarak yeniden yapılandırılır: Defter → Sayfa.
  - Sahiplik türleri **kişisel** (arkadaşlarla paylaşılabilir) ve **takım** olmak üzere ikidir. Ayrı bir "ortak defter" türü yoktur.
  - Düzenleme yetkisi sahiplik değildir. Bağlama, üye ekleme ve silme yetkileri sadece sahipte vardır.
  - Gizli bağlantıların varlığı hiçbir şekilde gösterilmez.
- **Sonuçlar:** Paylaşımın büyük kısmı defter düzeyinde, açık ve görünür şekilde olur. Sayfa düzeyinde miras yoluyla gelen erişim istisna olarak kalır ve öncesinde uyarı gösterilir.
- **Alternatifler:** Ayrı bir ortak defter türü. Paylaşım sırasında sayfaları taşımayı gerektireceği için reddedildi.

## ADR-0012 · Önce web, ama baştan mobile hazır
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Geliştirme ve test büyük ölçüde web üzerinde yapılır, ancak arayüz baştan React Native bileşenleriyle yazılır.
  - Sadece DOM'da çalışan kütüphaneler ve hover'a bağlı etkileşimler kullanılmaz.
  - Her epic sonunda iOS simülatöründe kısa bir doğrulama yapılır.
- **Sonuçlar:** Mobile geçerken arayüzü yeniden yazmak gerekmez.

## ADR-0013 · Dosya yükleme yok; embed
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Sayfalarda YouTube, Google Drive, Spotify, SoundCloud embed'leri ve link önizlemesi desteklenir.
  - Kullanıcıların dosya yüklemesine izin verilmez.
- **Sonuçlar:** Depolama maliyeti ve yükleme sırasında güvenlik taraması yükü oluşmaz. İleride kotalı dosya yükleme ücretli bir özellik olabilir.

## ADR-0014 · Kişisel kanban; görev ilerlemesi alt kırılımlardan hesaplanır
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Kanban sütunları ve görevlerin bu sütunlardaki yeri her kullanıcı için ayrıdır.
  - Görevin gerçek ilerlemesi sadece alt kırılımlardan hesaplanır.
  - Her görevin en az bir alt kırılımı olmak zorundadır; alt kırılım eklenmezse görev başlığıyla otomatik oluşturulur.
  - Birden fazla kişiye atanmış bir alt kırılımı, atananlardan biri işaretlediğinde tamamlanmış olur.
- **Sonuçlar:** Görevler hedeflere her zaman sayısal katkı verir. Bir kullanıcının panosunu düzenlemesi diğer kullanıcıları etkilemez.

## ADR-0015 · Work Session hiçbir şeyi tamamlamaz; ortak timer, kişisel log
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Work Session bağımsız bir modüldür.
  - Dilimler isteğe bağlı olarak bir nesneye bağlanır.
  - Süreler sadece istatistik olarak saklanır, hiçbir nesneyi kendiliğinden tamamlamaz.
  - Ortak oturumda timer durumunu sunucu tutar, bağlı nesneyi her katılımcı kendisi seçer.

## ADR-0016 · Kimlik doğrulama: kendi implementasyonumuz
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Şifreler argon2id ile hash'lenir.
  - Oturum token'ları opaktır ve veritabanında hash'li saklanır.
  - OAuth ile Google ve Apple girişi desteklenir.
  - Web'de cookie, mobil ve masaüstünde Bearer token kullanılır.
- **Sonuçlar:** Harici bir servise bağımlılık ve kullanıcı başı ücret oluşmaz. Bu alan güvenlik açısından kritik olduğu için yoğun şekilde test edilmelidir.
- **Alternatifler:**
  - Clerk/Auth0: maliyet ve bağımlılık nedeniyle reddedildi.
  - Keycloak/Zitadel: ayrı bir servisi işletme yükü nedeniyle reddedildi. Gerekirse yeniden değerlendirilebilir.

## ADR-0017 · Stil: NativeWind + kendi bileşen kütüphanemiz
**Durum:** Kabul · 2026-11-21

- **Karar:**
  - Stil için Tailwind tabanlı NativeWind kullanılır.
  - Tasarım token'ları ve bileşenler `packages/ui` altında tutulur.
  - Takım renkleri tema değişkenleriyle verilir.
- **Alternatifler:** Tamagui (daha karmaşık kurulum gerektirdiği için reddedildi), StyleSheet (tema yönetimi zahmetli olduğu için reddedildi).

## ADR-0018 · Kanban sürükle-bırak: Gesture Handler + Reanimated üzerinde ince bir katman
**Durum:** Kabul · 2026-11-22 (spike T0.6.1)

- **Bağlam:** Kişisel kanban (PRD `G6`) hem sütun içinde sıralama hem de sütunlar arası taşıma gerektiriyor. Aynı kod web'de (react-native-web) ve iOS'ta çalışmalı; sadece DOM'da çalışan kütüphaneler yasak (ADR-0012). ARCHITECTURE §18 bunu bilinen risk olarak işaretlemişti.
- **Karar:**
  - Hazır bir kanban/sürükle-bırak kütüphanesi kullanılmaz. `react-native-gesture-handler` (3.x, hook API: `usePanGesture`) ve `react-native-reanimated` (4.x) üzerinde ince, kendi katmanımız yazılır.
  - **Karar mantığı saf fonksiyonlardadır** (`apps/client/src/lib/dnd/board.ts`): bırakma hedefi (`dropTarget`), taşıma (`moveCard`), etkisiz bırakma (`isNoOp`), gösterge konumu (`indicatorPosition`). Birim testleriyle güvence altındadır; testler rastgele 500 taşımada kartların kaybolmadığını ve sıranın korunduğunu doğrular.
  - **Etkileşim:**
    - Dokunmatikte kart 250 ms basılı tutunca kalkar; böylece sütunlar normal kaydırmayla kayar.
    - Web'de fareyle birkaç piksel sürükleyince kart kalkar.
    - Kalkan kart, sütun kaydırma alanlarına kırpılmaması için panonun kökünde bir katmanda çizilir ve UI thread'de parmağı izler.
    - Kartın konumu ve sütun dikdörtgenleri sürükleme başlarken `measureInWindow` ile ölçülür. Bırakma hedefi hem hareket sırasında hem de bırakılan son noktadan hesaplanır; ölçüm bitmeden biten hızlı sürüklemelerde bırakma işlemi ölçümü bekler.
- **Sonuçlar:**
  - Prototip (`app/dev/kanban.tsx`, sadece geliştirme modunda) web'de ve iOS simülatöründe çalıştı: sütun içi sıralama ve sütunlar arası taşıma doğru konuma düşüyor.
  - Bağımlılık: `react-native-gesture-handler` eklendi ve kök layout `GestureHandlerRootView` ile sarıldı. Native modül olduğu için dev client yeniden derlenir.
  - T2.1.5 (kanban görünümü) bu katmanı kullanır. Prototipte olmayan ve T2.1.5'te yapılacaklar:
    - sürüklerken kenara gelince sütunun ve panonun otomatik kaydırılması,
    - uzun sütunlar için sanallaştırma (virtualization) ve buna uygun ölçüm,
    - erişilebilirlik alternatifi: ekran okuyucu ve klavye için "sütuna taşı" menüsü,
    - mobilde dokunsal geri bildirim (haptics).
  - Sunucuda kart sırası için kesirli sıralama anahtarı (fractional index) T2.1.5'te kararlaştırılır.
- **Alternatifler:**
  - `react-native-reanimated-dnd`: Draggable/Droppable ve tek liste sıralaması sunuyor ama sütunlar arası sıralı bırakma (ekleme konumu) yok; kanban için sütun başına ayrı bırakma alanları ve konum hesabını yine bizim yazmamız gerekirdi.
  - `react-native-sortables`: tek konteyner içinde güçlü, ama sütunlar arası taşıma o sürümde birinci sınıf desteklenmiyordu.
  - `react-native-draggable-flatlist`: yalnızca tek dikey liste.
  - `@dnd-kit` ve benzerleri: yalnızca DOM (ADR-0012 gereği reddedildi).

## ADR-0019 · Ortak düzenleme: y-sync sunucusu yrs'te, web'de TipTap, mobilde 10tap + soket aktarımı
**Durum:** Kabul · 2026-11-22 (spike T0.6.2; ADR-0005'i uygular)

- **Bağlam:** Defter sayfaları gerçek zamanlı ve kayıpsız ortak düzenlenecek (PRD `Y3`, ADR-0005). Mobilde editörün WebView içinde çalışması bilinen bir riskti (ARCHITECTURE §18). Spike'ta üç şey denendi: sunucu tarafında yrs ile y-sync, web'de iki tarayıcı, iOS'ta 10tap-editor.
- **Karar:**
  - **Sunucu** (`notebooks::collab`, `GET /collab/{page_id}`):
    - yrs 0.28 (`sync` özelliği) ile y-sync protokolü konuşulur: sync step 1/2, update ve awareness (canlı imleçler). Her sayfa için bellekte bir `Awareness` (Yjs belgesi) tutulur.
    - Belge ve awareness değişiklikleri gözlemcilerle o sayfanın tüm bağlantılarına yayınlanır.
    - Bağlantı kapanınca o bağlantının imleçleri silinir.
    - El sıkışma `/ws` gibi doğrulanır: bearer token, ya da web'de çerez ve izinli Origin.
    - İstemci tarafında standart `y-websocket` sağlayıcısı değişiklik yapmadan çalışır.
  - **Web:** TipTap (DOM) + `@tiptap/extension-collaboration` + `collaboration-caret`, `y-websocket` ile. Kod `CollabEditor.web.tsx` platform dosyasındadır. ADR-0012'deki "sadece DOM kütüphanesi yok" kuralının bilinçli istisnası budur: editör web'de DOM'da, mobilde WebView'da DOM'dadır; uygulamanın geri kalanı React Native bileşenleriyle kalır.
  - **Mobil:** 10tap-editor'ın "advanced setup"ı. `apps/client/editor-web/`, TenTap köprüleri ile TipTap Collaboration'ı tek bir HTML dosyasına derler (Vite + singlefile; `pnpm editor:build`). Bu dosya WebView'a `customSource` olarak verilir.
    - **Kimlik doğrulama sorunu:** WebView soketi kendisi doğrulayamaz: çerezi yoktur, Authorization başlığı gönderemez, Origin'i `null`'dır. Oturum token'ı WebView'a da girmemelidir.
    - **Çözüm: soket aktarımı.** WebView'daki y-websocket, 10tap köprüsü üzerinden çerçeveleri aktaran sahte bir soket kullanır (base64, JSON mesajlar). Gerçek `/collab` bağlantısını React Native bearer token ile açar. Her aktarılan bağlantının WebView'ın seçtiği bir kimliği vardır; eski kimliğe ait mesajlar yok sayılır. Bu, sayfa açılışta değiştiğinde eski soketin kapanışının yeni bağlantıyı düşürmesini engelliyor.
  - **10tap'in web paketi kaynaktan derlenir:** `@10play/tentap-editor/web` kendi TipTap kopyasını içinde taşıyor; Collaboration eklentisi farklı bir `@tiptap/core` görünce çöküyor (`editor.utils` undefined). Vite'ta `@10play/tentap-editor/src/webEditorUtils`'e alias verilip `@tiptap/core`, `@tiptap/pm`, `yjs` ve React tekilleştirilir. TenTap'in geçmiş (undo/redo) köprüsü kullanılmaz; geçmişi Yjs tutar.
- **Sonuçlar:**
  - Uçtan uca çalıştı: iki tarayıcı aynı sayfayı düzenledi; sonradan katılan güncel içeriği aldı; canlı imleçler göründü. iOS simülatörü ile tarayıcı arasında iki yönlü canlı düzenleme ve imleçler çalıştı.
  - Mobil editör paketi yaklaşık 1 MB (gzip ~310 KB). Üretilen `editorHtml.js` repoya girmez; derleme betiği EAS/üretim yapılarında çalıştırılmalı (E2.5 / dağıtım işi).
  - Web ↔ iOS gecikmesi simülatörde 1–2 saniyenin altında gözlemlendi; köprüde base64 ve JSON maliyeti var. Gerçek cihazda ölçülmesi E2.5'te.
  - **Spike'ta olmayan, E2.5'te yapılacaklar:**
    - sayfa yetkisi (VIEW/EDIT, salt okunur mod) ve `PermissionService`,
    - kalıcılık (`page_updates`, snapshot, sürüm geçmişi; ADR-0005),
    - odaların bellekten atılması,
    - `AccessRevoked` ile bağlantı kapatma (V10),
    - mobilde araç çubuğunun klavyeyle birlikte görünmesi (spike'ta klavye araç çubuğunu kapatıyor),
    - BP4: kopma sonrası yerel değişikliklerin gönderilmesi; y-websocket ve Yjs bunu doğal olarak yapar, kilit katmanıyla birlikte doğrulanmalı.
  - Uç nokta spike'ta sadece `COLLAB_SPIKE_ENABLED=true` iken bağlanıyordu; T2.5.2 yetki ve kalıcılığı ekleyince bayrak kaldırıldı ve uç nokta her zaman bağlanır.
- **Alternatifler:**
  - Token'ı URL'de ya da `Sec-WebSocket-Protocol` başlığında WebView'a vermek: token WebView'a ve loglara sızar; reddedildi.
  - Yjs'i React Native tarafında çalıştırıp WebView'a sadece ProseMirror adımlarını göndermek: iki taraflı ProseMirror ↔ Yjs eşlemesini yeniden yazmak gerekir; reddedildi.
  - Mobilde yerel (native) bir zengin metin editörü: TipTap şemasıyla uyum ve Yjs bağlaması yok; reddedildi.
  - Hazır y-sync sunucusu (Hocuspocus, Node): ikinci bir dil ve süreç demek, yetki ve kalıcılığın Rust tarafında olması gerekiyor (ADR-0005, ADR-0007); reddedildi.

## ADR-0020 · Masaüstü: Tauri 2 paketlenmiş web çıktısı, bearer oturum, WebSocket Rust eklentisinden
**Durum:** Kabul · 2026-11-22 (spike T0.6.3; ADR-0002'yi uygular)

- **Bağlam:** Masaüstü uygulaması web çıktısını Tauri 2 ile paketleyecek (ADR-0002). Web istemcisi oturumu `HttpOnly; Secure; SameSite=Lax` bir çerezle, `/ws` el sıkışmasını da çerez ve izinli Origin ile doğruluyor. Paketlenmiş uygulama kendi origin'inden çalıştığı için bu yaklaşımın masaüstünde çalışıp çalışmadığı belirsizdi.
- **Ölçüm** (`apps/desktop`, macOS, Tauri 2.12, WKWebView; `app/dev/desktop.tsx` tanılama ekranı sonuçları stdout'a raporlar):
  - Uygulama macOS'ta açılıyor; sayfa `tauri://localhost/...` origin'inden yükleniyor.
  - Sunucu CORS listesi `tauri://localhost`'u kabul etmediği için tüm istekler engellendi ve uygulama "Bağlantı yok" ekranında kaldı. Bu yüzden #97 açıldı.
  - API ve uygulama aynı sitedeyken (`http://localhost:8080`) çerez de bearer da çalıştı: WebKit `tauri://localhost` ile `http://localhost`'u aynı site sayıyor.
  - **Üretimdeki gibi farklı sitede** (API `http://127.0.0.1:8080`):
    - çerez modu: kayıt 201, `/me` **401**, `/ws` **reddedildi** (çerez siteler arası isteklerde gönderilmiyor),
    - bearer modu: `/me` **200**,
    - WebView'ın kendi WebSocket'i Authorization header gönderemediği için reddedildi,
    - Tauri'nin WebSocket eklentisi (Rust, `tauri-plugin-websocket`) header ile bağlandı ve `hello` aldı.
- **Karar:**
  - Masaüstü, web çıktısını **paketler** (uzaktaki web adresini yüklemez) ve oturumu **bearer token** ile taşır. ARCHITECTURE §15'teki "mobil ve masaüstü: Authorization: Bearer" kuralı ölçümle doğrulandı. Çerez yaklaşımı masaüstünde kullanılmaz.
  - `/ws` ve `/collab` bağlantıları masaüstünde `tauri-plugin-websocket` ile Rust tarafından açılır. Bağlantı katmanındaki `SocketFactory` soyutlamasına bir Tauri uyarlaması yazılır.
  - Token işletim sisteminin güvenli deposunda saklanır (macOS Keychain, Windows Credential Manager, Linux Secret Service); E4.1'de bir Tauri komutuyla yapılır.
  - Sunucu Tauri origin'lerini (`tauri://localhost`, `http(s)://tauri.localhost`) CORS'ta kabul eder (#97). Bearer istekleri çerez taşımadığı için bu origin'lerin çerezli istek yetkisi kazanması risk oluşturmaz.
- **Sonuçlar:**
  - İstemcide oturum taşıma seçimi üç platforma çıkar: web için çerez, iOS/Android için bearer (secure-store), masaüstü için bearer (Keychain) ve Rust WebSocket. Tauri ortamı `__TAURI_INTERNALS__` ile algılanır.
  - Debug `.app` yaklaşık 38 MB; release boyutu ve imzalama/notarization E4.1'de.
  - Spike iskeleti `apps/desktop` (Tauri 2 kabuğu, ikonlar, WebSocket izni) E4.1'in başlangıç noktasıdır. `spike_report` komutu ve `/dev/desktop` tanılama ekranı yalnızca geliştirme içindir ve E4.1'de kaldırılır.
- **Alternatifler:**
  - Tauri penceresinde uzaktaki web uygulamasını yüklemek: çerezler birinci taraf olur ve istemci değişmez. Ama Tauri IPC'si uzak içeriğe açılır ve sürüm masaüstü yayınından kopar; güvenlik nedeniyle reddedildi.
  - Çerezi `SameSite=None` yapmak: WebKit'in üçüncü taraf çerez engeli (ITP) yine engeller; web'in CSRF korumasını da zayıflatır. Reddedildi.
  - Token'ı WebSocket URL'sinde ya da `Sec-WebSocket-Protocol` başlığında göndermek: token loglara sızabilir; Rust eklentisi header gönderebildiği için gerek yok. Reddedildi.
  - Electron: ADR-0002'de reddedildi; spike bu kararı değiştirecek bir sorun göstermedi.

## ADR-0021 · Barındırma: Türkiye'de tek VDS, tek ortam, ortak edge proxy, merge'de otomatik deploy
**Durum:** Kabul · 2026-11-22 (sahibin onayıyla; ARCHITECTURE §17'deki ayrı "staging/beta" ortamının yerine geçer)

- **Bağlam:**
  - E0.7 için bir sunucu ve alan adı gerekiyordu.
  - Sahip Türkiye'de bir VDS aldı (4 çekirdek, 6 GB, 50 GB SSD, Debian 13) ve `routune.dev` alan adını Cloudflare'den kaydetti. Uygulamanın adı **routune**.
  - Sahip aynı sunucuda başka Docker servislerini (ör. n8n) de çalıştırmak istiyor.
  - Gerçek kullanıcı gelene kadar ayrı bir staging ortamına gerek görmüyor: canlı ortam şimdilik staging işlevi görecek.
- **Karar:**
  - **Tek ortam:** `main`'e giren her değişiklik canlıya deploy edilir (`.github/workflows/deploy.yml`). Dallar `main`'e girmeden hiçbir şey canlıya çıkmaz.
  - **Modüler sunucu:**
    - `/opt/edge` (`infra/edge`): 80/443'ü dinleyen tek container olan ortak Caddy. TLS'i (Let's Encrypt) o sağlar ve `sites/*.caddy` dosyalarını okur.
    - Her uygulama ayrı bir Compose klasöründe çalışır (`/opt/routune` = `infra/production`, ileride `/opt/n8n`…). Dış `edge` Docker ağına benzersiz bir adla bağlanır ve kendi veritabanını kendi iç ağında tutar.
  - **Deploy:**
    - İmajlar GitHub Actions'ta derlenir ve SSH ile `deploy` kullanıcısına kopyalanır. Registry kullanılmaz.
    - Sunucuda `deploy.sh` imajları yükler, `current` olarak etiketler, servisleri `--wait` ile başlatır, `/health` kontrolü yapar ve son 3 sürümü geri alma için saklar.
    - Sırlar sunucudaki `/opt/routune/.env` dosyasında durur. Deploy anahtarı ve host bilgisi GitHub secrets'tadır.
  - **Sunucu güvenliği** (`infra/server/bootstrap.sh`): sadece anahtarla SSH, ufw (22/80/443), fail2ban, otomatik güvenlik güncellemeleri.
  - DNS Cloudflare'de, kayıtlar **DNS only**. Cloudflare proxy'si kullanılmaz; TLS ve WebSocket doğrudan Caddy'de sonlanır.
- **Sonuçlar:**
  - Veri Türkiye'de kalır. Sunucu tarafında KVKK yurt dışı aktarım sorunu doğmaz. Yedeklerin nereye alınacağı (T0.7.3) bu açıdan ayrıca değerlendirilir.
  - Canlıda geliştirme yapıldığı için migration'lar geriye uyumlu yazılır (genişlet → taşı → daralt). Geri alma imaj etiketiyle yapılır, migration geri alınmaz.
  - Gerçek bir e-posta sağlayıcısı (aday: Resend) bağlanana kadar sunucu `APP_ENV=staging` ve `EMAIL_DRIVER=log` ile çalışır.
  - Gerçek kullanıcılar gelince ayrı bir staging ortamı, aynı yapıya ikinci bir uygulama klasörü olarak eklenebilir (`/opt/routune-staging`, başka alt alan adları).
  - Paylaşılan sunucuda kaynaklar ortak. Başka servisler routune'u etkilememeli; gerekirse Compose'ta bellek sınırı verilir.
- **Alternatifler:**
  - Ayrı staging ve production: şimdilik gereksiz maliyet ve bakım. Gerçek kullanıcılarla birlikte yeniden değerlendirilecek.
  - Dokploy veya Coolify gibi bir panel: n8n kurulumu kolaylaşır, ama panel 0,5–1 GB bellek kullanır ve deploy mantığı repodan panele taşınır. Reddedildi; ileride görsel izleme için hafif bir araç (Portainer) eklenebilir.
  - Container registry (GHCR) ile pull: paket görünürlüğü ve sunucuya ek bir token gerekir. SSH ile imaj aktarımı tek sunucu için daha sade.
  - Cloudflare proxy'si (turuncu bulut): DDoS koruması sağlar, ama TLS modu, WebSocket ve gerçek istemci IP'si için ek ayar gerekir. Gerekirse ileride açılır.
  - Hetzner gibi AB merkezli sağlayıcılar: daha ucuz, ama KVKK yurt dışı aktarım yükü getiriyor.

## ADR-0022 · İşlemsel e-posta: Resend
**Durum:** Kabul · 2026-11-22 (sahibin seçimi; ARCHITECTURE §14'teki açık kararı kapatır)

- **Bağlam:** Doğrulama ve şifre sıfırlama e-postaları şimdiye kadar sadece loga yazılıyordu (`EMAIL_DRIVER=log`). Canlı ortamda (ADR-0021) gerçek gönderim gerekiyor. Sahip Resend'i seçti; kendi mail sunucusunu kurmak da ileride düşünülebilir.
- **Karar:**
  - `kernel::email::ResendEmailSender`, Resend HTTP API'sine (`POST https://api.resend.com/emails`, Bearer anahtar) düz metin e-posta gönderir. Modüller sağlayıcıyı bilmez; `EmailSender` arayüzü değişmez.
  - Yapılandırma: `EMAIL_DRIVER=resend`, `RESEND_API_KEY` (sır, sunucudaki `.env`), `EMAIL_FROM` (ör. `Routune <noreply@routune.dev>`). Eksikse sunucu açılmaz. `resend` sürücüsü `production`'da izinlidir.
  - Gönderici alan adı `routune.dev`, Resend'de doğrulanır (Cloudflare'de SPF/DKIM kayıtları).
  - Kişisel veri: anahtar, alıcı ve gövde loglara ve hata mesajlarına girmez. Hata sadece HTTP durum kodunu ya da bağlantı hatasının türünü taşır.
  - HTTP istemcisi `reqwest` 0.13 (rustls); sentry zaten aynı sürümü kullandığı için ek bir TLS yığını gelmez.
- **Sonuçlar:**
  - Resend kayıtları KVKK açısından yurt dışına aktarım sayılır (alıcı adresi ve e-posta içeriği ABD'deki sağlayıcıya gider). Gerçek kullanıcılar gelmeden önce aydınlatma metninde belirtilmeli; gerekirse AB bölgesi ya da Türkiye'de bir sağlayıcı seçilebilir. Sürücü arayüzü sayesinde sağlayıcı değişikliği tek bir dosyada kalır.
  - Gönderim şimdilik istek içinde, senkron yapılır (mevcut davranış). Sağlayıcı yavaşlarsa veya hata verirse, ileride gönderim `jobs` kuyruğuna taşınıp yeniden denenebilir.
- **Alternatifler:**
  - SMTP (`lettre`) ile kendi sunucumuz ya da başka bir sağlayıcı: alan adı itibarı, IP ısındırma ve spam yönetimi gerektirir. Erken aşamada gereksiz yük.
  - Postmark, SES, Mailgun: benzer yetenekler; sahip Resend'i tercih etti.

## ADR-0023 · Dağıtım ve güncellemeler: mağazasız beta, masaüstünde otomatik güncelleme, mobilde OTA
**Durum:** Kabul · 2026-11-22 (sahibin onayıyla; ROADMAP F4'teki E4.1 ve E4.2'nin bir kısmını öne çeker, T4.2.5'teki EAS Build'in yerine geçer)

- **Bağlam:**
  - Sahip uygulamayı mağazalara yüklemeden, kurulum paketleriyle arkadaşlarına dağıtmak istiyor.
  - Ücretli Apple Developer hesabı yok ve şimdilik alınmayacak.
  - Güncellemelerin olabildiğince tek merkezden yönetilmesi isteniyor: web zaten her merge'de güncelleniyor (ADR-0021), masaüstü Discord gibi kendini güncellemeli.
- **Karar:**
  - **Masaüstü:**
    - Tauri paketleri (`.dmg`, `.exe`, AppImage) GitHub Actions'ta üretilip GitHub Releases'a yüklenir (T4.1.3).
    - Uygulama `tauri-plugin-updater` ile açılışta güncelleme kontrolü yapar, imzalı paketi indirir, kurar ve yeniden başlar (T4.1.4). Güncelleme imza anahtarı Tauri'nin kendi anahtarıdır (GitHub secret); Apple ya da Windows sertifikası gerekmez.
    - Paketler imzasızdır; ilk kurulumdaki işletim sistemi uyarısı beta notlarında anlatılır.
  - **Mobil paketler:** EAS Build yerine kendi CI'ımız kullanılır (T4.2.5).
    - Android: kalıcı bir anahtarla imzalı APK. Yeni sürümler eskisinin üstüne kurulabilir.
    - iOS: simülatör `.app` zip'i ve imzasız `.ipa`. Kullanıcı imzasız IPA'yı Sideloadly/AltStore ile kendi ücretsiz Apple ID'siyle imzalar; bu imza 7 günde bir yenilenir.
  - **Mobil OTA:** JS ve arayüz güncellemeleri `expo-updates` ile kurulu uygulamaya uzaktan gelir. Güncellemeler kendi sunucumuzdan (Expo Updates protokolü) ve deploy hattından yayınlanır (T4.2.6). Native değişiklikler runtime sürümünü artırır ve yeni paket gerektirir.
  - **Minimum sürüm:** Sunucu platform başına desteklenen en düşük istemci sürümünü bildirir; eski uygulamalar yeni sürümü indirme ekranı gösterir (T4.2.7).
  - **Masaüstünde oturum:** ADR-0020'deki bearer token ve Rust WebSocket kararı T4.1.1'de uygulanır. Token işletim sisteminin güvenli deposunda tutulur (`keyring` crate'i).
- **Sonuçlar:**
  - Kullanıcılar çoğu güncellemeyi yeniden kurulum yapmadan alır. Yeni APK/IPA sadece native değişikliklerde gerekir.
  - Kendi OTA sunucumuz bir uç nokta ve deploy adımı daha demek; karşılığında üçüncü taraf hesabı gerekmez ve veri bizde kalır.
  - İmzasız iOS dağıtımı geniş kitleye uygun değildir; geniş bir beta ya da mağaza aşamasında Apple Developer Program (TestFlight) yeniden değerlendirilir.
  - Mağazaya ilk yüklemeden önce bundle id'ler (`app.secondbrain.*`) gözden geçirilmeli; mağazaya çıktıktan sonra değiştirilemezler.
- **Alternatifler:**
  - EAS Build ve EAS Update: kurulumu hızlı, ama Expo hesabı ve üçüncü taraf hizmeti gerektirir. Kendi CI ve sunucumuz tercih edildi; gerekirse geçiş kolaydır.
  - TestFlight ve notarization: ücretli Apple hesabı gerektirir; şimdilik reddedildi.
  - Masaüstünde uzak web adresini yüklemek (her zaman güncel): ADR-0020'de güvenlik nedeniyle reddedildi.

## ADR-0024 · Web öncelikli geliştirme; platform paketleri ve native özellikler en sona
**Durum:** Kabul · 2026-11-22 (sahibin onayıyla; ADR-0023'ün altyapısını korur, sürüm çıkarma zamanlamasını değiştirir)

- **Bağlam:**
  - Uygulama tüm fazlar bitince arkadaşlara beta olarak gönderilecek. O zamana kadar ihtiyaç, işlevleri çalışan bir uygulama.
  - Her platformda ayrı kontrol ve ara sürüm çıkarmak iş yükünü ve maliyeti artırıyor.
  - Ücretli Apple Developer hesabı yok ve alınmayacak (ADR-0023).
- **Karar:**
  - Geliştirme ve doğrulama web'de yapılır. `main`'e düzenli merge edilir; sahip küçük kontrolleri https://app.routune.dev üzerinden yapar.
  - Masaüstü (`desktop-v*`) ve mobil (`mobile-v*`) paketleri tüm fazlar bitene kadar çıkarılmaz. İlk beta paketleri en sonda üretilir. ADR-0023'teki CI, otomatik güncelleme, OTA ve minimum sürüm altyapısı olduğu gibi kalır.
  - Platform kontrolü (iOS simülatörü, Android, masaüstü) sadece kritik ve platforma özgü davranışlarda yapılır; epic kapanışındaki kontrol web'dedir.
  - Native özellikler ve platform işleri en sona bırakılır ve board'da `zamanlama:en-son` etiketiyle işaretlenir: masaüstü bildirimleri (T4.1.2), mobil uyumluluk turu (T4.2.1), mobil editör (T4.2.2), Android push (T4.2.3), Android'de Google ile giriş (T0.4.11), uygulama içi geri bildirim (T4.3.4).
  - Ücretli Apple Developer hesabı gerektiren hiçbir iş yapılmaz (ör. iOS push, TestFlight). iOS imzasız IPA ile desteklenmeye devam eder.
  - Mobil push sadece Android'de, Firebase Cloud Messaging ile yapılır; ayrıntılı karar T4.2.3'te ayrı bir ADR olarak yazılır.
  - Android'de Apple ile giriş sunulmaz; Android'deki seçenekler e-posta + şifre ve Google'dır.
  - Beta geri bildirimleri şimdilik GitHub issue'larıyla toplanır.
- **Sonuçlar:**
  - İş yükü ve CI maliyeti düşer; geri bildirim döngüsü web üzerinden hızlanır.
  - Platforma özgü hatalar geç fark edilebilir. Bunu azaltmak için kritik native davranışlar yine kontrol edilir ve beta öncesine bir uyumluluk turu (T4.2.1) konur.
  - React Native bileşen kuralı (ADR-0012) sürdüğü için arayüz kodu platformlara taşınabilir kalır.
- **Alternatifler:**
  - Her epic sonunda iOS simülatöründe kontrol ve düzenli ara sürümler: zaman ve maliyet nedeniyle şimdilik reddedildi.
