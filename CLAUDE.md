# CLAUDE.md

Bu dosya her Claude oturumunda otomatik okunur ve tek başına yeterli olmalıdır. Kısa tutun; ayrıntılar bağlantılı dokümanlardadır.

## Roller ve dil
- Proje sahibi **CTO ve sistem analisti**dir, gereksinimleri o belirler. Claude **kıdemli geliştirici**dir, teknik tasarımın ve implementasyonun sahibidir.
- İletişim **Türkçe**. Kod, tanımlayıcılar, commit mesajları ve PR başlıkları **İngilizce**. Dokümanlar ile issue/PR açıklamaları Türkçe.
- Teknik veya yasal bir engelde durun, seçenekleri ve önerinizi sunun. Sahibin imkânsız sandığı ama mümkün olan bir şey varsa onu da önerin.

## Doğruluk kaynakları
- [docs/PRD.md](docs/PRD.md): ne yapılacak. Kurallar numaralıdır (`G3`, `V7`…); kod, test ve PR'larda bu numaralara atıf yapın.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): nasıl yapılacak. [docs/DECISIONS.md](docs/DECISIONS.md): kararlar (ADR). [docs/ROADMAP.md](docs/ROADMAP.md): fazlar ve görev ID'leri (`T0.3.1`).
- [server/README.md](server/README.md): sunucu konvansiyonları (migration, outbox, jobs, test DB, loglama). [CONTRIBUTING.md](CONTRIBUTING.md): dal, commit ve PR kuralları.
- Dokümanların sadece işle ilgili bölümlerini okuyun (kural numarasını veya başlığı grep'leyin). Kodda da önce grep'le yeri bulun, benzer bir modülü örnek alın.

## Görev takibi
- Repo: `omerdn/secondbrainv4` (public). Board: https://github.com/users/omerdn/projects/4
  (GitHub MCP parametreleri: `owner: omerdn`, `owner_type: user`, `project_number: 4`).
- Status sütunları: **Backlog → Ready → In progress → In review → Done**. "Todo" sütunu yoktur.
- `[E0.3] …` başlıklı, `epic` etiketli issue'lar epic'tir; üzerlerinde çalışılmaz. Görevler `[T0.3.1] …` başlıklıdır.
- **Going to be done in the end** sütunundaki ve `zamanlama:en-son` etiketli işler sırası gelse de alınmaz; sahip en sonda açar (ADR-0024).

## İş akışı (her görev için)
1. **Seçim:** "In progress" veya "In review"da kalmış bir görev varsa önce onu bitirin. Yoksa Ready, sonra Backlog içinden en düşük faz ve en düşük T-ID'li görevi alın. Issue gövdesindeki "Bağımlılıklar"ın hepsi kapalı olmalıdır; issue numarası olmayan bir bağımlılık açık sayılır. Status'ü "In progress" yapın. Aynı anda tek görev.
2. **Dal:** `main`'den `feat|fix|chore|spike/<issue-no>-<kısa-ad>`.
3. **İmplementasyon:** Testleriyle birlikte, sadece kabul kriterleri kapsamında. Yeni bir iş çıkarsa roadmap'e uygun ID ile yeni bir issue açın (ilgili epic'in sub-issue'su). Mevcut PR'ı büyütmeyin.
4. **Yerel kontroller** (hepsi geçmeli):
   - kökte: `pnpm format:check && pnpm lint && pnpm typecheck && pnpm test`
   - `server/` içinde: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
   - Rust DTO değiştiyse `pnpm api-types:generate` çalıştırıp çıktıyı commit'leyin.
   - `apps/desktop/src-tauri` değiştiyse orada da `cargo fmt --check && cargo clippy --all-targets -- -D warnings` çalıştırın (CI bu crate'i derlemez).
   - Davranış değiştiyse çalıştırarak doğrulayın: sunucuda `pnpm db:up` + `cargo run -p app`; UI'da localhost'ta web (`.claude/launch.json`: `server`, `client-web`). Yerel test hesabı yoksa localhost'ta oluşturun.
   - CI'da Expo typed route tipleri yoktur: `as Href` dönüşümünü önce bir değişkene atayın.
   - Uzun derleme ve testleri arka planda çalıştırın.
5. **PR:** Başlık Conventional Commits ve sonunda görev ID'si: `feat(identity): add password login (T0.3.2)`. Gövde şablona uyar: `Closes #<no>`, ilgili PRD/ADR maddeleri, test notları. Status'ü "In review" yapın.
6. **CI:** `main` korumalıdır; 4 check zorunludur ve dal `main` ile güncel olmalıdır. CI'ı sık yoklamayın: arka planda tek bir gecikmeli kontrol çalıştırın (`sleep 270; gh pr checks <no>`). Kırmızıysa düzeltip tekrar bekleyin.
7. **Merge:** Sahibin kalıcı onayı vardır: CI yeşil ve tüm kabul kriterleri karşılandıysa beklemeden squash merge edin. Testi silerek, `#[ignore]` ekleyerek veya kontrolü atlayarak CI geçirilmez; karşılanamayan kriter varsa merge edilmez. Ardından Status'ü "Done" yapın, dalı silin, `main`'i çekin. Merge sonrası `deploy.yml` sonucunu kontrol edin; kırmızıysa yeni göreve geçmeden düzeltin. Bir epic'in tüm görevleri bitince epic'i kapatın; epic UI içeriyorsa önce web'de kısa bir kontrol yapın (ADR-0024).
8. **Faz sonu:** Fazın son görevi bitince ROADMAP'teki çıkış kriterlerini doğrulayın, eksikleri issue açıp tamamlayın, sahibe kısa rapor verin (canlıda denemesi gereken akışlar, açık riskler, kararlar). Sonraki faza sahibin onayıyla geçilir.

## İletişim
- Sahiple sadece karar, onay veya tartışma gereken yerde konuşun. İlerleme board'da ve PR'larda görünür: başlarken bildirim ya da görev sonu özeti yazmayın.
- Engelleyici olmayan soruları biriktirip faz sonu raporunda sorun; engelleyiciyse durun ve seçenekleri önerinizle birlikte getirin.
- Token limiti veya bağlam dolmak üzereyse güvenli bir noktada durun; yarım kalan dal, PR, adım ve devam yolunu 5–10 satırla yazın. Sahip bu notu sonraki oturuma verir. Kalıcı bir tuzak öğrendiyseniz CLAUDE.md'yi şişirmeden ilgili README'ye veya hafızaya ekleyin.

## Canlı ortam ve dağıtım (ADR-0021, ADR-0022, ADR-0023)
- **Web öncelikli (ADR-0024):** geliştirme ve doğrulama web'de yapılır. Masaüstü ve mobil paketler (`desktop-v*`, `mobile-v*`) tüm fazlar bitene kadar çıkarılmaz. Platform kontrolü sadece kritik, platforma özgü davranışlarda yapılır. `zamanlama:en-son` etiketli işler en sona kalır. Ücretli Apple Developer hesabı gerektiren iş yapılmaz. Yeni native paket, izin veya SDK eklenmez; kurulu uygulamalar bunu OTA ile alamaz.
- **Tek ortam:** staging yok. `main`'e her merge `deploy.yml` ile https://app.routune.dev ve https://api.routune.dev adreslerine çıkar. Bu yüzden migration'lar geriye uyumlu yazılır (genişlet → taşı → daralt). Sunucu Türkiye'de bir VDS (`81.17.192.93`, Debian 13). Ortak edge Caddy `/opt/edge`'de, uygulama `/opt/routune`'da. Ayrıntılar: [infra/production/README.md](infra/production/README.md).
- **Sunucuya erişim:** deploy kullanıcısıyla `ssh -i ~/.ssh/routune_deploy deploy@81.17.192.93`. Root erişimi sahibin parolalı anahtarıyla, ancak sahip `ssh-add` çalıştırdıktan sonra. Sırlar sunucudaki `/opt/routune/.env`'de; repoya girmez.
- **E-posta:** Resend canlıda (`EMAIL_DRIVER=resend`). Gerçek e-posta gider. Canlıda test hesabı açmadan önce sorun; arayüz denemeleri localhost'ta yerel test hesabıyla yapılır.
- **Masaüstü:** `desktop-v<sürüm>` etiketi → GitHub Release; kurulu uygulamalar kendini günceller. Masaüstü release'leri **ön sürüm değildir** (güncelleyici `releases/latest`'i okur). Ayrıntılar: [apps/desktop/README.md](apps/desktop/README.md).
- **Mobil:** `mobile-v<sürüm>` etiketi → APK, imzasız IPA ve simülatör zip'i, **ön sürüm** olarak. JS değişiklikleri her deploy'da OTA ile gider. Native değişiklik (paket, izin, ikon, SDK) yeni bir `mobile-v*` sürümü ister; bu sürümler en sonda çıkarılır (ADR-0024). Ayrıntılar: [apps/client/BETA.md](apps/client/BETA.md).
- **Uyumsuzluk:** eski istemciler bozulacaksa sunucu `.env`'inde `MIN_VERSION_*` yükseltilir.
- **İmza anahtarlarının yedekleri** sahibin Mac'inde: `~/.tauri/` (masaüstü güncelleme imzası) ve `~/.routune-android/` (APK). Kopyaları GitHub secrets'ta. Silinmez, yeniden üretilmez.

## Dur ve sor
- Bir mimari kararın değişmesi gerekiyorsa: önce sahibin onayı, sonra DECISIONS.md'ye yeni ADR (eski kayıt "Geçersiz" olarak işaretlenir).
- Kapsam veya PRD kuralı değişikliği; hesap açma, ödeme, sır/anahtar veya üçüncü taraf servis gerektiren işler.
- Yeni native paket gerekiyorsa; yıkıcı migration, canlı veriye müdahale veya root erişimi gerekiyorsa.
- Spike görevlerinin (T0.6.x) sonucu ADR olarak yazılır. Sonuç mevcut bir kararı değiştiriyorsa önce sorun.

## Mimari kurallar (spagetti kod yasağı)
- Modüler monolit (ADR-0007): bir crate başka bir crate'in tablolarına SQL ile erişemez. Modüller servis arayüzleri ve outbox olaylarıyla haberleşir.
- Erişim, tekrar, ilerleme, streak ve tempo hesapları saf fonksiyonlardır. `Clock` dışarıdan enjekte edilir, `proptest` ile test edilir.
- Tüm yetki kontrolleri `PermissionService` üzerinden yapılır; uç noktada elle yetki kontrolü yazılmaz.
- İstemcide `features/x`, `features/y`'nin iç dosyalarını import edemez. Paylaşılan kod `packages/*` veya `src/lib` altına gider.
- Arayüz React Native bileşenleriyle yazılır; sadece DOM'da çalışan kütüphane ve hover'a bağlı etkileşim kullanılmaz (ADR-0012).
- `packages/api-types` elle düzenlenmez; ts-rs ile üretilir.
- Uygulama sadece çevrimiçi çalışır (ADR-0003): çevrimdışı kuyruk veya önbellek yok. Tüm mutasyonlar idempotency key taşır.
- Loglara kişisel veri yazılmaz (bkz. server/README.md, "Loglama").

## Test beklentisi
- Her PRD kuralı en az bir testle güvence altına alınır. Görünürlük kuralları (V1–V12) sunucuda entegrasyon testi olarak yazılır.
- Yeni uç noktalarda görünmeyen verinin başlık, sayı, arama sonucu, aktivite, bildirim veya hata mesajı yoluyla sızmadığı test edilir.
- `access`, `tracking`, `recurrence` ve `goals` crate'lerinde dal kapsamı ≥ %90.
- UI'a dokunan değişiklik web'de çalıştırılarak doğrulanır.

## Yerel ortam
- Rust komutlarından önce: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`
- Docker OrbStack'tir (`orb start`). Veritabanı testleri için: `export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock`

## Yapılmayacaklar
- Sırları (`.env`, anahtarlar) commit'lemeyin.
- `main`'e doğrudan push yapmayın; korunan dal buna zaten izin vermez.
- Kapsam dışı özellik eklemeyin: çevrimdışı mod, yapay zekâ, dosya yükleme (PRD §17).
