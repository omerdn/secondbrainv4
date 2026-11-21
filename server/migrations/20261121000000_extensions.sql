-- Extensions required by global search (ARCHITECTURE §2, §12): trigram matching and accent
-- folding. Both are "trusted" extensions (PostgreSQL 13+), so a non-superuser database owner can
-- create them. Installed into `public` explicitly so every schema (including per-test schemas,
-- which put `public` on their search_path) can use them.
CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;
CREATE EXTENSION IF NOT EXISTS unaccent WITH SCHEMA public;
