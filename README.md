# secondbrainv4

Görevleri, rutinleri, hedefleri, sayısal takipleri ve defterleri **tek bir merkezde, birbirine bağlı** yöneten; kişisel, arkadaşla ortak ve takım kullanımını tek bir workspace'te destekleyen bir "hayat işletim sistemi".

> Durum: **Planlama tamamlandı, geliştirme Faz 0'dan başlıyor.**

## Dokümanlar
| Doküman | İçerik |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Ürün gereksinimleri ve numaralı kurallar |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Stack, repo yapısı, veri modeli, yetki modeli, hesap formülleri |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Mimari karar kayıtları (ADR) |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Fazlar, epic'ler, görevler |
| [CLAUDE.md](CLAUDE.md) | Geliştirme iş akışı ve kurallar |

## Stack (özet)
- **Arayüz:** Expo (React Native) + react-native-web. iOS, Android ve web tek kod tabanından çıkar.
- **Masaüstü:** Tauri 2
- **Sunucu:** Rust (axum, sqlx), PostgreSQL
- **Gerçek zamanlı düzenleme:** Yjs + yrs

## Geliştirme

### Gereksinimler
- Node.js ≥ 24 (önerilen sürüm `.nvmrc` içinde) ve pnpm 12
- rustup: Rust sürümü `rust-toolchain.toml` ile sabittir. macOS'ta `brew install rustup`, ardından `PATH`'e `/opt/homebrew/opt/rustup/bin` eklenir.
- Docker (Docker Desktop veya OrbStack) ve Docker Compose v2

### İlk kurulum
```bash
cp .env.example .env   # yerel ayarlar; .env git'e girmez
pnpm install
pnpm db:up             # Postgres 17'yi başlatır ve hazır olana kadar bekler
pnpm db:check          # sürümü ve pg_trgm/unaccent uzantılarını doğrular
```

### Yerel veritabanı
Postgres 17, `compose.yaml` ile çalışır ve sadece `127.0.0.1:5432` adresinden erişilebilir. Bağlantı adresi `.env` içindeki `DATABASE_URL` değişkenindedir. İlk açılışta `infra/postgres/init/` altındaki betik `pg_trgm` ve `unaccent` uzantılarını hem uygulama veritabanına hem `template1`'e kurar.

```bash
pnpm db:up      # başlat
pnpm db:down    # durdur (veriler korunur)
pnpm db:psql    # psql oturumu aç
pnpm db:check   # sağlık kontrolü
pnpm db:reset   # DİKKAT: yerel veritabanını tamamen siler ve sıfırdan kurar
```

### Komutlar
```bash
pnpm lint        # ESLint (tüm paketler)
pnpm typecheck   # tsc --noEmit (tüm paketler)
pnpm test        # Vitest (tüm paketler)
pnpm dev         # geliştirme sunucuları (istemci T0.4.1 ile eklenecek)
pnpm format      # Prettier
```

Repo yapısı ([ARCHITECTURE §3](docs/ARCHITECTURE.md#3-repo-yapısı)):

| Klasör | İçerik |
|---|---|
| `apps/` | Expo istemcisi ve Tauri masaüstü kabuğu |
| `packages/config` | Paylaşılan ESLint, Prettier ve tsconfig ayarları (`@secondbrain/config`) |
| `server/` | Rust Cargo workspace (pnpm workspace'inin parçası değil). Komutlar: [server/README.md](server/README.md) |

### CI
Her PR'da ve `main`'e her push'ta [`.github/workflows/ci.yml`](.github/workflows/ci.yml) çalışır. Üç iş vardır: **Rust** (fmt, clippy, test), **TypeScript** (format, lint, typecheck, test) ve **Dev environment** (Compose ile Postgres + `db-check`). pnpm deposu, Cargo derlemeleri ve Turborepo çıktıları önbelleğe alınır. Bir PR, üç iş de yeşil olmadan merge edilmez.

Görevler Turborepo ile paketler arasında çalıştırılır. Yeni bir TS paketi eklerken `@secondbrain/config`'ten `eslint.config.js`, `tsconfig.json` (`extends: "@secondbrain/config/tsconfig/base.json"`) devralınır ve `lint`, `typecheck`, `test` script'leri tanımlanır.

## Görev takibi
GitHub Project: https://github.com/users/omerdn/projects/4
