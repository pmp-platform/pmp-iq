-- migrate:up
-- Entity kinds become first-class like properties: a stable id, a friendly
-- name, and a description. `value` is renamed to `kind_id`; name/description are
-- added (name backfilled from the id). Properties gain a description too.
ALTER TABLE entity_kinds RENAME COLUMN value TO kind_id;
ALTER TABLE entity_kinds ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_kinds ADD COLUMN description TEXT NOT NULL DEFAULT '';
UPDATE entity_kinds SET name = kind_id WHERE name = '';

ALTER TABLE entity_properties ADD COLUMN description TEXT NOT NULL DEFAULT '';

-- migrate:down
