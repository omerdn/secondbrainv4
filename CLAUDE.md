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
- Dokümanların sadece işle ilgili bölümlerini okuyun.

## Görev takibi
- Repo: `omerdn/secondbrainv4` (public). Board: https://github.com/users/omerdn/projects/4
  (GitHub MCP parametreleri: `owner: omerdn`, `owner_type: user`, `project_number: 4`).
- Status sütunları: **Backlog → Ready → In progress → In review → Done**. "Todo" sütunu yoktur.
- `[E0.3] …` başlıklı, `epic` etiketli issue'lar epic'tir; üzerlerinde çalışılmaz. Görevler `[T0.3.1] …` başlıklıdır.

## İş akışı (her görev için)
1. **Seçim:** "In progress" veya "In review"da kalmış bir görev varsa önce onu bitirin. Yoksa Ready, sonra Backlog içinden en düşük faz ve en düşük T-ID'li görevi alın. Issue gövdesindeki "Bağımlılıklar"ın hepsi kapalı olmalıdır. Sahibe tek satırla bildirin, Status'ü "In progress" yapın. Aynı anda tek görev.
2. **Dal:** `main`'den `feat|fix|chore|spike/<issue-no>-<kısa-ad>`.
3. **İmplementasyon:** Testleriyle birlikte, sadece kabul kriterleri kapsamında. Yeni bir iş çıkarsa roadmap'e uygun ID ile yeni bir issue açın (ilgili epic'in sub-issue'su). Mevcut PR'ı büyütmeyin.
4. **Yerel kontroller** (hepsi geçmeli):
   - kökte: `pnpm format:check && pnpm lint && pnpm typecheck && pnpm test`
   - `server/` içinde: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
   - Rust DTO değiştiyse `pnpm api-types:generate` çalıştırıp çıktıyı commit'leyin.
   - Davranış değiştiyse çalıştırarak doğrulayın: sunucuda `pnpm db:up` + `cargo run -p app`, UI'da web.
5. **PR:** Başlık Conventional Commits ve sonunda görev ID'si: `feat(identity): add password login (T0.3.2)`. Gövde şablona uyar: `Closes #<no>`, ilgili PRD/ADR maddeleri, test notları. Status'ü "In review" yapın.
6. **CI:** `main` korumalıdır; 4 check zorunludur ve dal `main` ile güncel olmalıdır. CI'ı sık yoklamayın: arka planda tek bir gecikmeli kontrol çalıştırın (`sleep 270; gh pr checks <no>`). Kırmızıysa düzeltip tekrar bekleyin.
7. **Merge:** Sahibin onayıyla yapılır. Sahip oturum hedefinde (`/goal`) onay verdiyse CI yeşilken merge edin. Squash merge kullanın. Ardından Status'ü "Done" yapın, dalı silin, `main`'i çekin. Bir epic'in tüm görevleri bitince epic'i kapatın; epic UI içeriyorsa önce iOS simülatöründe kısa bir kontrol yapın.

## Dur ve sor
- Bir mimari kararın değişmesi gerekiyorsa: önce sahibin onayı, sonra DECISIONS.md'ye yeni ADR (eski kayıt "Geçersiz" olarak işaretlenir).
- Kapsam değişikliği; hesap açma, ödeme veya sır/anahtar gerektiren işler.
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
- `access`, `tracking`, `recurrence` ve `goals` crate'lerinde dal kapsamı ≥ %90.
- UI'a dokunan değişiklik web'de çalıştırılarak doğrulanır.

## Yerel ortam
- Rust komutlarından önce: `export PATH=/opt/homebrew/opt/rustup/bin:$PATH`
- Docker OrbStack'tir (`orb start`). Veritabanı testleri için: `export DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock`

## Yapılmayacaklar
- Sırları (`.env`, anahtarlar) commit'lemeyin.
- `main`'e doğrudan push yapmayın; korunan dal buna zaten izin vermez.
- Kapsam dışı özellik eklemeyin: çevrimdışı mod, yapay zekâ, dosya yükleme (PRD §17).
