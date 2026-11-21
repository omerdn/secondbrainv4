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

## Komutlar
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
