# syntax=docker/dockerfile:1
# Caddy image with the static web client (T0.7.1, ARCHITECTURE §17). Build context: repository
# root. EXPO_PUBLIC_API_URL is baked into the bundle at build time.

FROM node:26-slim AS build
# Same pnpm as package.json "packageManager".
RUN npm install -g pnpm@12.11.2
WORKDIR /src
COPY . .
RUN pnpm install --frozen-lockfile --filter "@secondbrain/client..."
ARG EXPO_PUBLIC_API_URL
RUN test -n "$EXPO_PUBLIC_API_URL" || { echo "EXPO_PUBLIC_API_URL build arg is required" >&2; exit 1; }
RUN cd apps/client && pnpm exec expo export --platform web --output-dir /web

FROM caddy:2.10-alpine
COPY infra/staging/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /web /srv/web
