# ADR 0004: SQLite access via `rusqlite` (bundled) + `rusqlite_migration`

## Status
Accepted

## Context
Echora needs local persistence (settings, history, favorites, moods
metadata, cache index). Candidates: `rusqlite` (sync) vs. `sqlx` (async,
compile-time-checked queries, multi-database).

Echora has no need for async DB access (it's a single-user local desktop
app, not a service handling concurrent connections) or multi-database
support. `sqlx` pulls more transitive dependencies and async-runtime
coupling for capability Echora doesn't use. `rusqlite` with its
`bundled` feature compiles SQLite from source into the binary — no
system `libsqlite3` dependency, which matters for the "zero install"
requirement.

## Decision
Use `rusqlite` with the `bundled` feature, and `rusqlite_migration` for
schema migrations (uses SQLite's `user_version`, no extra tracking
table, no CLI tool required).

## Consequences
- Fewer transitive dependencies, no async overhead for DB calls.
- Self-contained: the SQLite library is compiled into Echora's own
  binary, satisfying "zero install" without relying on any system
  package.
- If a future need for concurrent/async DB access ever arises, this
  decision would need revisiting — not expected for this product shape.
