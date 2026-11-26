-- Global search (PRD AR1, ARCHITECTURE §12, T2.7.5). Additive only.
--
-- A search document joins two readings of the text: Turkish stems with their accents then
-- folded ("çiçeği" → çiçek → cicek), and the folded text stemmed ("Kitaplari" → kitap), so a
-- query with or without Turkish letters and with or without suffixes finds the same words, and
-- "ışık", "işık" and "isik" are one word. A query is read the same two ways. `unaccent` is only
-- STABLE, so it is called with an explicit dictionary inside IMMUTABLE functions, usable in
-- index expressions. Titles also get a trigram index for typos and parts of words. Each index
-- is on its owner's table and is queried only by that owner: `items` (titles, descriptions) and
-- `notebooks` (page text) (ADR-0007).

-- The functions live in the migrated schema (like the tables), so per-test schemas each get
-- their own; their bodies name the extension's objects in `public` explicitly.

CREATE FUNCTION search_unaccent(text) RETURNS text
    LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
    AS $$ SELECT public.unaccent('public.unaccent'::regdictionary, $1) $$;

-- Bodies call only `pg_catalog` and the extension in `public`: maintenance (ANALYZE, REINDEX)
-- evaluates index expressions with a restricted search_path.

CREATE FUNCTION search_document(text) RETURNS tsvector
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    AS $$ SELECT to_tsvector('pg_catalog.simple'::regconfig,
                             public.unaccent('public.unaccent'::regdictionary,
                                array_to_string(tsvector_to_array(
                                    to_tsvector('pg_catalog.turkish'::regconfig, coalesce($1, ''))), ' ')))
              || to_tsvector('pg_catalog.turkish'::regconfig,
                             public.unaccent('public.unaccent'::regdictionary, coalesce($1, ''))) $$;

CREATE FUNCTION search_query(text) RETURNS tsquery
    LANGUAGE sql IMMUTABLE PARALLEL SAFE
    AS $$ SELECT websearch_to_tsquery('pg_catalog.simple'::regconfig,
                                      public.unaccent('public.unaccent'::regdictionary,
                                array_to_string(tsvector_to_array(
                                    to_tsvector('pg_catalog.turkish'::regconfig, coalesce($1, ''))), ' ')))
              || websearch_to_tsquery('pg_catalog.turkish'::regconfig,
                                      public.unaccent('public.unaccent'::regdictionary, coalesce($1, ''))) $$;

-- Owned by `items`.
CREATE INDEX items_search_idx ON items
    USING gin (search_document(title || ' ' || coalesce(description, '')));
CREATE INDEX items_title_trgm_idx ON items
    USING gin (search_unaccent(lower(title)) public.gin_trgm_ops);

-- Owned by `notebooks`.
CREATE INDEX page_snapshots_search_idx ON page_snapshots
    USING gin (search_document(plain_text));
