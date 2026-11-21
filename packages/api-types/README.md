# @secondbrain/api-types

Rust DTO'larından [ts-rs](https://github.com/Aleph-Alpha/ts-rs) ile **üretilen** TypeScript tipleri (ADR-0002, T0.2.7).

**Elle düzenlenmez.** `src/generated/` ve `src/index.ts` üretilir:

```bash
pnpm api-types:generate   # kökten: cargo test (ts-rs export) + index.ts
```

Bir Rust tipini dışa aktarmak için: `#[derive(TS)]` ve `#[ts(export)]`. CI, üretilen dosyalar commit'lenenden farklıysa başarısız olur.
