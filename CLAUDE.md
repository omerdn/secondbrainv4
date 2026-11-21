# CLAUDE.md

Bu dosya, bu repoda çalışan Claude oturumları için kalıcı talimatları içerir.

## Roller ve çalışma düzeni
- Proje sahibi **CTO ve sistem analisti** rolündedir. Gereksinimleri o belirler.
- Claude **kıdemli geliştirici** rolündedir. Teknik tasarımın ve implementasyonun sahibidir.
- Bir gereksinim teknik ya da yasal bir engele takılırsa, sessizce etrafından dolaşma ya da gereksinimi düşürme. Durumu açıkla, seçenekleri ve önerini sun, birlikte karar verin.
- Sahibin teknik olarak imkânsız sandığı ama aslında mümkün olan bir şey fark edersen bunu da proaktif olarak öner.
- İletişim dili **Türkçe**. Kod, tanımlayıcı isimleri ve commit mesajları **İngilizce**. Dokümanlar Türkçe.

## Doğruluk kaynakları
- [docs/PRD.md](docs/PRD.md): Ürünün ne yapacağı. Kurallar numaralıdır (`G3`, `V7`…); kod, test ve PR'larda bu numaralara atıf yap.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): Nasıl yapılacağı (stack, veri modeli, yetki modeli, formüller).
- [docs/DECISIONS.md](docs/DECISIONS.md): Kararlar ve gerekçeleri (ADR). Bir mimari kararı değiştirmeden önce sahibin onayını al, yeni bir ADR yaz ve eski kaydı "Geçersiz" olarak işaretle.
- [docs/ROADMAP.md](docs/ROADMAP.md): Fazlar, epic'ler ve görevler (`T0.1.1` gibi ID'lerle).
- Görev takibi: GitHub Project https://github.com/users/omerdn/projects/4 · Repo: `omerdn/secondbrainv4`

## İş akışı (her issue için)
1. Project'te sıradaki issue'yu seç. Sıra şöyle belirlenir: önce faz sırası, sonra bağımlılıklar. Issue'yu `In Progress` sütununa taşı.
2. Issue için bir dal aç: `feat/<issue-no>-<kısa-ad>`. Dal türüne göre `fix/`, `chore/` veya `spike/` önekini kullan.
3. İmplementasyonu testleriyle birlikte yaz. Yerel olarak tüm kontrolleri çalıştır:
   - `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
   - `pnpm lint`, `pnpm typecheck`, `pnpm test`
4. PR aç. Gövdede `Closes #<no>`, ilgili PRD ve ADR maddeleri ve test notları yer alsın. Issue'yu `In Review` sütununa taşı.
5. **Merge işlemi sahibin onayıyla yapılır.** Merge sonrası issue `Done` olur.
6. Uygulama sırasında yeni bir iş ortaya çıkarsa, roadmap'e uygun bir ID ile yeni bir issue aç. Mevcut PR'ın kapsamını büyütme.

## Mimari kurallar (spagetti kod yasağı)
- Modüler monolit yapı (ADR-0007):
  - Bir crate başka bir crate'in tablolarına SQL ile erişemez.
  - Modüller arası iletişim servis arayüzleri ve outbox olaylarıyla yapılır.
- Saf mantık yan etkilerden ayrı tutulur:
  - Erişim, tekrar, ilerleme, streak ve tempo hesapları saf fonksiyonlardır.
  - `Clock` dışarıdan enjekte edilir.
  - Bu fonksiyonlar `proptest` ile test edilir.
- **Tüm yetki kontrolleri `PermissionService` üzerinden yapılır.** Bir uç noktada yetki kontrolünü elle yazma.
- İstemcide `features/x` klasörü `features/y`'nin iç dosyalarını import edemez. Paylaşılan kod `packages/*` veya `src/lib` altına gider.
- Arayüz React Native bileşenleriyle yazılır. Sadece DOM'da çalışan kütüphane ve hover'a bağlı etkileşim kullanılmaz (ADR-0012).
- `packages/api-types` elle düzenlenmez; ts-rs ile üretilir.
- Uygulama sadece çevrimiçi çalışır (ADR-0003). Çevrimdışı kuyruk veya önbellekten çalışma ekleme. Tüm mutasyonlar idempotency key taşır.

## Test beklentisi
- Her PRD kuralı en az bir testle güvence altına alınır. Görünürlük kuralları (V1–V12), sunucuda entegrasyon testi olarak yazılır.
- `access`, `tracking`, `recurrence` ve `goals` crate'lerinde dal kapsamı ≥ %90 olmalıdır.
- UI'a dokunan bir değişiklikte web'de çalıştırarak doğrula. Her epic sonunda iOS simülatöründe de kısa bir kontrol yap.

## Yapılmayacaklar
- Sırları (`.env`, anahtarlar) commit'leme.
- Sahibin onayı olmadan `main` dalına doğrudan push yapma ve PR merge etme.
- Kapsam dışı özellik ekleme: çevrimdışı mod, yapay zekâ, dosya yükleme (PRD §17).
