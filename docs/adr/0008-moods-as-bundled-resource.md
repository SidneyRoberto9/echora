# ADR 0008: Moods are bundled static data, not database rows

## Status
Accepted

## Context
Echora ships at least 50 (target 60) predefined moods, each with a name,
category, trait scores, and search queries. This data changes with app
releases (new moods, retuned traits/queries), not through user action at
runtime.

Two options: seed it into SQLite via a migration, or bundle it as a
versioned resource file loaded into memory at startup.

## Decision
Moods live in `src-tauri/resources/moods.json`, embedded into the binary
via `include_str!` and parsed once into an in-memory `MoodCatalog` at
startup (see `src/moods.rs`). They are never written to SQLite.

## Consequences
- Updating a mood (new query, retuned trait) is a code change shipped in
  the next release, not a migration — no risk of migration drift between
  what the app expects and what's in a user's existing database.
- No seed-data migration to write or keep idempotent.
- User-specific data that *references* moods (favorites, session
  history) stores the mood's `id` as a plain string column, not a
  foreign key into a `moods` table — validated against the loaded
  `MoodCatalog` at the point of use instead of by the database schema.
