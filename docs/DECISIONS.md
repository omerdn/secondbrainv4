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
  - Uç nokta şu an sadece `COLLAB_SPIKE_ENABLED=true` iken bağlanır (varsayılan kapalı). Staging ve üretimde açılmaz.
- **Alternatifler:**
  - Token'ı URL'de ya da `Sec-WebSocket-Protocol` başlığında WebView'a vermek: token WebView'a ve loglara sızar; reddedildi.
  - Yjs'i React Native tarafında çalıştırıp WebView'a sadece ProseMirror adımlarını göndermek: iki taraflı ProseMirror ↔ Yjs eşlemesini yeniden yazmak gerekir; reddedildi.
  - Mobilde yerel (native) bir zengin metin editörü: TipTap şemasıyla uyum ve Yjs bağlaması yok; reddedildi.
  - Hazır y-sync sunucusu (Hocuspocus, Node): ikinci bir dil ve süreç demek, yetki ve kalıcılığın Rust tarafında olması gerekiyor (ADR-0005, ADR-0007); reddedildi.
