# Katkı rehberi

Bu repoda çalışırken uyulan kurallar. Genel iş akışı ve mimari kurallar [CLAUDE.md](CLAUDE.md)'de, ürün ve mimari kararlar `docs/` altında.

## Dil

- Kod, tanımlayıcılar, commit mesajları ve PR başlıkları: **İngilizce**
- Dokümanlar, issue ve PR açıklamaları: **Türkçe**

## Dallar

Her issue için ayrı bir dal açılır: `<tür>/<issue-no>-<kısa-ad>`

| Önek     | Ne zaman                                 |
| -------- | ---------------------------------------- |
| `feat/`  | Yeni özellik                             |
| `fix/`   | Hata düzeltme                            |
| `chore/` | Altyapı, araçlar, bakım                  |
| `spike/` | Teknik keşif (sonucu ADR olarak yazılır) |

Örnek: `feat/21-email-password-auth`

## Commit mesajları: Conventional Commits

Format ([conventionalcommits.org](https://www.conventionalcommits.org/)):

```
<type>(<scope>)?: <özet>

<gövde: ne ve neden>

Closes #<no>
```

- **type:** `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `perf`, `ci`, `build`, `revert`
- **scope (opsiyonel):** etkilenen crate, paket veya özellik: `access`, `identity`, `api-client`, `tasks`…
- **özet:** emir kipinde, küçük harfle başlar, sonunda nokta yoktur. Roadmap ID'si sona eklenebilir: `feat(identity): add password login (T0.3.2)`
- **Geriye uyumsuz değişiklik:** type'tan sonra `!` konur (`feat(api)!: …`) ve gövdeye `BREAKING CHANGE:` satırı eklenir.

PR'lar **squash merge** edilir. Bu yüzden `main`'e giren commit mesajı **PR başlığıdır**. PR başlığının bu formata uyduğu CI'da (`PR title` kontrolü) doğrulanır.

## Pull request

1. PR açıklaması şablona göre doldurulur: `Closes #<no>`, ilgili PRD ve ADR maddeleri, test notları, kontrol listesi.
2. Yerel kontroller geçmeden PR açılmaz:
   ```bash
   pnpm format:check && pnpm lint && pnpm typecheck && pnpm test
   (cd server && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace)
   ```
3. CI'daki tüm kontroller yeşil olmadan merge yapılmaz.
4. Bir PR tek bir issue'yu kapsar. Çalışırken yeni bir iş çıkarsa roadmap'e uygun ID ile yeni bir issue açılır.

## Issue

Yeni issue'lar **Görev** veya **Hata** şablonuyla açılır. Görev başlıkları roadmap ID'siyle başlar: `[T0.1.5] PR ve issue şablonları`.
