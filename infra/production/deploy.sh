#!/bin/sh
# Rolls out a new version on the host (ADR-0021). Run by .github/workflows/deploy.yml as the
# `deploy` user from /opt/routune after it uploaded the files and images/*.tar.gz:
#   ./deploy.sh <tag>
# Rollback: docker tag routune-server:<old> routune-server:current (same for web), then
#   docker compose up -d --wait
set -eu

TAG=${1:?usage: deploy.sh <tag>}
APP_DIR=$(cd "$(dirname "$0")" && pwd)
EDGE_DIR=${EDGE_DIR:-/opt/edge}
KEEP_IMAGES=3
# HEALTH_INSECURE=1 skips certificate checks (local runs with Caddy's internal CA only).

cd "$APP_DIR"
[ -f .env ] || { echo ".env is missing in $APP_DIR (see README.md)" >&2; exit 1; }
env_value() { sed -n "s/^$1=//p" .env | tail -n 1; }
WEB_DOMAIN=$(env_value WEB_DOMAIN)
API_DOMAIN=$(env_value API_DOMAIN)
[ -n "$WEB_DOMAIN" ] && [ -n "$API_DOMAIN" ] || { echo "WEB_DOMAIN/API_DOMAIN missing in .env" >&2; exit 1; }

echo "==> loading images ($TAG)"
for archive in images/*.tar.gz; do
	gunzip -c "$archive" | docker load
done
rm -f images/*.tar.gz
docker tag "routune-server:$TAG" routune-server:current
docker tag "routune-web:$TAG" routune-web:current

echo "==> edge proxy"
docker compose -f "$EDGE_DIR/compose.yaml" up -d --wait

echo "==> app"
docker compose up -d --wait --remove-orphans

echo "==> edge site"
sed -e "s/__WEB_DOMAIN__/$WEB_DOMAIN/g" -e "s/__API_DOMAIN__/$API_DOMAIN/g" \
	site.caddy.template > "$EDGE_DIR/sites/routune.caddy.tmp"
mv "$EDGE_DIR/sites/routune.caddy.tmp" "$EDGE_DIR/sites/routune.caddy"
docker compose -f "$EDGE_DIR/compose.yaml" exec -T caddy caddy reload --config /etc/caddy/Caddyfile

echo "==> health check"
ok=""
for _ in $(seq 1 30); do
	if curl -fsS ${HEALTH_INSECURE:+-k} --resolve "$API_DOMAIN:443:127.0.0.1" "https://$API_DOMAIN/health" > /dev/null 2>&1; then
		ok=1
		break
	fi
	sleep 2
done
[ -n "$ok" ] || { echo "https://$API_DOMAIN/health did not answer" >&2; exit 1; }

echo "==> pruning old images (keeping $KEEP_IMAGES)"
for repo in routune-server routune-web; do
	docker image ls "$repo" --format '{{.Tag}} {{.CreatedAt}}' \
		| grep -v '^current ' | sort -k2 -r | awk "NR>$KEEP_IMAGES {print \$1}" \
		| while read -r old; do docker image rm "$repo:$old" > /dev/null || true; done
done
docker image prune -f > /dev/null

echo "==> deployed $TAG"
