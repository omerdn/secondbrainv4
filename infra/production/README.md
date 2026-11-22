# Canlı ortam (routune.dev, ADR-0021)

Tek ortam vardır: `main`'e giren her değişiklik canlıya deploy edilir. Sunucu Türkiye'de bir VDS'tir (Debian 13). Yapı modülerdir: ortak bir edge proxy ve her uygulama için ayrı bir Compose klasörü.

```
/opt/edge/          infra/edge          Caddy: 80/443'ü dinleyen tek container, tüm siteler için TLS.
                                        sites/*.caddy dosyalarını okur. Ortak "edge" Docker ağı.
/opt/routune/       infra/production    Uygulama: server + web + postgres.
                                        Edge'e routune.caddy site dosyasını yazar.
/opt/<başka-uygulama>/                  Örneğin n8n: kendi compose'u ve /opt/edge/sites/n8n.caddy.
```

| Servis     | İmaj                                       | Görev                                                                                     |
| ---------- | ------------------------------------------ | ----------------------------------------------------------------------------------------- |
| `postgres` | `postgres:17`                              | Sadece `internal` ağında. Port açmaz, edge'den erişilemez.                                |
| `server`   | `server/Dockerfile` → `routune-server`     | Rust sunucu. Migration'lar binary'ye gömülüdür, açılışta uygulanır. Edge'deki adı `routune-server`. |
| `web`      | `web.Dockerfile` → `routune-web`           | Statik web istemcisi (TLS'siz Caddy, :80). Edge'deki adı `routune-web`.                   |

Alan adları: `app.routune.dev` (web) ve `api.routune.dev` (API ve `/ws`). Aynı site olduklarından oturum cookie'si (`SameSite=Lax`) gönderilir. CORS `PUBLIC_WEB_URL` origin'ine izin verir.

## Deploy (otomatik)

`.github/workflows/deploy.yml`, `main`'e her push'ta şunları yapar:

1. İki imajı GitHub Actions'ta derler. Registry kullanılmaz.
2. İmajları ve `infra/edge`, `infra/production` dosyalarını SSH ile `deploy` kullanıcısına kopyalar.
3. `/opt/routune/deploy.sh <commit>` çalıştırır. Betik imajları yükler ve `current` olarak etiketler, edge'i ve uygulamayı başlatır, site dosyasını yazıp Caddy'yi yeniden yükler, `/health`'i kontrol eder ve son 3 sürüm dışındaki imajları siler.
4. Dışarıdan `https://api.routune.dev/health` kontrolü yapar.

Workflow'un ihtiyaç duyduğu GitHub secrets: `DEPLOY_HOST`, `DEPLOY_SSH_KEY` (`deploy` kullanıcısının özel anahtarı), `DEPLOY_KNOWN_HOSTS`. Elle tetiklemek için: Actions → Deploy → Run workflow.

**Geri alma:** sunucuda `docker image ls routune-server` ile eski commit etiketini bulun, sonra:

```bash
cd /opt/routune
docker tag routune-server:<eski> routune-server:current
docker tag routune-web:<eski> routune-web:current
docker compose up -d --wait
```

Migration'lar geri alınmaz. Bu yüzden migration'lar **geriye uyumlu** yazılır: önce kolon eklenir, eski kod kaldırıldıktan sonra silinir.

## Sunucu kurulumu (bir kez)

1. `ssh root@<ip> 'sh -s' < infra/server/bootstrap.sh`. Betik şunları kurar ve ayarlar:
   - paket güncellemeleri ve otomatik güvenlik güncellemeleri,
   - Docker (resmi depo),
   - güvenlik duvarı (ufw: 22, 80, 443) ve fail2ban,
   - `deploy` kullanıcısı (docker grubunda),
   - `/opt/edge` ve `/opt/routune` klasörleri.

   Ayrıca **şifreyle SSH girişini kapatır**. Bu yüzden betiği sadece anahtarla bağlanabildiğinizi doğruladıktan sonra çalıştırın.
2. Deploy anahtarının açık kısmını `/home/deploy/.ssh/authorized_keys` dosyasına ekleyin.
3. `/opt/routune/.env` dosyasını `.env.example`'dan oluşturun: alan adlarını girin, `POSTGRES_PASSWORD=$(openssl rand -hex 32)` ile şifre üretin. Dosya `deploy` kullanıcısına ait olmalı ve izni `600` olmalı.
4. Cloudflare DNS'te `app` ve `api` için A kaydını sunucu IP'sine yönlendirin, **DNS only** (gri bulut). Caddy sertifikayı Let's Encrypt'ten kendisi alır.

## Sunucuya başka bir uygulama eklemek (ör. n8n)

1. `/opt/n8n/compose.yaml` oluşturun. Container'ı dış `edge` ağına benzersiz bir adla bağlayın (`networks: { edge: { aliases: [n8n] } }`, `networks: { edge: { external: true } }`). Kendi veritabanını kendi iç ağında tutun.
2. `/opt/edge/sites/n8n.caddy` dosyasını yazın (örnek: `infra/edge/sites/example.caddy.txt`). Ardından `docker compose -f /opt/edge/compose.yaml exec caddy caddy reload --config /etc/caddy/Caddyfile` ile Caddy'yi yeniden yükleyin.
3. DNS'e `n8n` A kaydını ekleyin.

`/opt/edge/sites/` altında routune'un deploy'u sadece `routune.caddy` dosyasına dokunur.

## Notlar

- `EXPO_PUBLIC_API_URL` web paketine derleme sırasında gömülür (workflow'daki `API_URL`).
- `POSTGRES_PASSWORD` `DATABASE_URL`'in içine girer. Bu yüzden sadece harf ve rakam kullanın.
- Gerçek bir e-posta sağlayıcısı gelene kadar `APP_ENV=staging` kullanılır. `EMAIL_DRIVER=log` `production`'da reddedilir. Bu sürede doğrulama ve sıfırlama bağlantıları sadece sunucu logunda görünür.
- Loglar container başına 10 MB × 5 dosyayla sınırlıdır. Caddy sertifikaları `caddy_data` volume'undadır; bu volume'u silmeyin.
- Yedekleme: T0.7.3 (#45).

## Yerelde deneme

```bash
docker network create edge
docker build -f server/Dockerfile -t routune-server:current .
docker build -f infra/production/web.Dockerfile --build-arg EXPO_PUBLIC_API_URL=https://api.localhost -t routune-web:current .
cp infra/production/.env.example infra/production/.env   # WEB_DOMAIN=app.localhost, API_DOMAIN=api.localhost
POSTGRES_INIT_DIR=../postgres/init docker compose -f infra/production/compose.yaml up -d --wait
```

Edge için `infra/edge/sites/routune.caddy` dosyasını şablondan üretip `docker compose -f infra/edge/compose.yaml up -d` çalıştırın. `*.localhost` sertifikaları Caddy'nin yerel CA'sından gelir, bu yüzden `curl -k --resolve api.localhost:443:127.0.0.1 https://api.localhost/health` ile kontrol edin.
