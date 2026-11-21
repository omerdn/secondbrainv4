<!--
PR başlığı Conventional Commits formatında ve İngilizce olmalı (squash merge sonrası main'deki commit mesajı olur):
  feat(identity): add password login (T0.3.2)
Bkz. CONTRIBUTING.md
-->

Closes #

## Özet

<!-- Ne değişti ve neden? Roadmap görevi (ör. T0.3.2) ve epic. -->

## İlgili maddeler

- PRD: <!-- ör. G3, V7 -->
- ARCHITECTURE: <!-- ör. §6.3 -->
- ADR: <!-- ör. ADR-0004 -->

## Test notları

<!-- Hangi testler eklendi, neyi güvence altına alıyor? Yerelde ne çalıştırıldı? UI değişikliği varsa web'de nasıl doğrulandı? -->

## Kontrol listesi

- [ ] Yerel kontroller geçti: `pnpm format:check`, `pnpm lint`, `pnpm typecheck`, `pnpm test`
- [ ] Rust değiştiyse: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
- [ ] Değişen her PRD kuralı en az bir testle güvence altında
- [ ] Yetki kontrolleri sadece `PermissionService` üzerinden (elle yetki kontrolü yok)
- [ ] Modül sınırları korunuyor: başka crate'in tablosuna SQL yok, `features/x` → `features/y` iç import yok
- [ ] Mutasyonlar idempotency key taşıyor; çevrimdışı kuyruk veya önbellek eklenmedi (ADR-0003)
- [ ] UI değiştiyse web'de çalıştırılarak doğrulandı
- [ ] `packages/api-types` elle düzenlenmedi
- [ ] Sır (`.env`, anahtar, token) commit'lenmedi
- [ ] Gerekiyorsa README/dokümanlar güncellendi; mimari karar değiştiyse yeni ADR yazıldı
