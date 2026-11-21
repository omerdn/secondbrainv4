-- Runs once, when the dev database volume is first initialised (connected to $POSTGRES_DB).
-- Extensions required by search (ARCHITECTURE §2, §12): trigram matching and accent folding.
-- Installed into template1 too, so databases created later (e.g. for tests) inherit them.
-- Note: application migrations (T0.2.3) must still `CREATE EXTENSION IF NOT EXISTS` so that
-- staging/production do not depend on this script.

-- 1) The application database (the entrypoint's current connection).
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS unaccent;

-- 2) template1, inherited by every database created afterwards.
\connect template1
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS unaccent;
