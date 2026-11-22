# Staging / beta yığını (T0.7.1)

Tek bir sunucuda Docker Compose ile üç servis çalışır (ARCHITECTURE §17):

| Servis     | İmaj                                                     | Görev                                                                                    |
| ---------- | -------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `postgres` | `postgres:17`                                            | Veritabanı. Dışarıya port açmaz, sadece iç ağdan erişilir.                               |
| `server`   | `server/Dockerfile` → `secondbrain-server`               | Rust sunucu. Migration'lar binary'ye gömülüdür ve açılışta uygulanır.                    |
| `caddy`    | `infra/staging/web.Dockerfile` → `secondbrain-caddy`     | TLS, `WEB_DOMAIN`'de statik web istemcisi, `API_DOMAIN`'de sunucuya ters proxy (`/ws` dahil). |

İki alan adı kullanılır, örneğin `app.ornek.com` (web) ve `api.ornek.com` (API). Aynı kök alan adının altında oldukları için oturum cookie'si (`SameSite=Lax`) gönderilir, CORS da `PUBLIC_WEB_URL` origin'ine izin verir.

## Çalıştırma

```bash
cd infra/staging
cp .env.example .env   # alan adlarını ve POSTGRES_PASSWORD'ü doldurun; .env commit'lenmez
docker compose up -d --build --wait
docker compose logs -f server
```

- `EXPO_PUBLIC_API_URL` web paketine **derleme sırasında** gömülür (`https://$API_DOMAIN`). `API_DOMAIN` değişirse `caddy` imajı yeniden derlenmelidir.
- `POSTGRES_PASSWORD` `DATABASE_URL`'in içine girer. URL'de sorun çıkarmaması için sadece harf ve rakam kullanın (`openssl rand -hex 32`).
- Caddy sertifikaları `caddy_data` volume'unda tutulur. Bu volume'u silmeyin, Let's Encrypt'in istek sınırları var.
- Sunucu `TRUST_PROXY_HEADERS=true` ile çalışır: hız sınırları istemci IP'sini Caddy'nin yazdığı başlıklardan alır. Caddy, istemcinin gönderdiği `X-Forwarded-For`, `X-Real-IP` ve `Forwarded` başlıklarını kendi değerleriyle ezer.
- Masaüstü uygulaması için `CORS_EXTRA_ORIGINS` örnekte hazırdır (T0.6.4).
- VPS kurulumu, otomatik deploy (T0.7.2) ve yedekleme (T0.7.3) ayrı görevlerdir.

## Yerelde deneme

`.env.example` olduğu gibi kullanılınca yığın bu makinede `https://app.localhost` ve `https://api.localhost` adreslerinde açılır. `*.localhost` sertifikaları Caddy'nin yerel CA'sından gelir, bu yüzden `curl -k` kullanın:

```bash
curl -k --resolve api.localhost:443:127.0.0.1 https://api.localhost/health
curl -k --resolve app.localhost:443:127.0.0.1 -I https://app.localhost/
```

Kapatmak için `docker compose down` (verileri de silmek için `-v`).

## İmajları ayrı derleme

Build context'i depo köküdür:

```bash
docker build -f server/Dockerfile -t secondbrain-server .
docker build -f infra/staging/web.Dockerfile --build-arg EXPO_PUBLIC_API_URL=https://api.ornek.com -t secondbrain-caddy .
```

`server/Dockerfile` içindeki Rust imaj sürümü `rust-toolchain.toml` ile aynı tutulmalıdır.
