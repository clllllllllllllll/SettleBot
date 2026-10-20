# Persistence

## Boundaries

[Repository contracts](../src/persistence/repositories.rs) separate roster, game, draft, session/entry, processed-update, and atomic-confirmation storage from the [use cases](../src/persistence/use_cases.rs). The [D1 adapter](../src/persistence/d1/mod.rs) accepts an injected `worker::D1Database`; the pure domain and use cases do not depend on Workers. The HTTP entry point has no write routes.

Persistent drafts hold a validated domain selection, numeric owner, partial amounts, revision and state. Every database read rebuilds validated identifiers, names, dates and participant selections. Session reads also run the original domain confirmation checks. Malformed records return a typed corruption error; storage failures do not become successful results.

## Schema and identities

[0001](../migrations/0001_ledger.sql) creates strict tables for players, games, drafts, draft participants, sessions, session entries and processed updates. Primary and foreign keys preserve stable identities; participant identity includes its kind and owning draft/session. Roster and guest IDs with identical text remain distinct. Guest identities and names stay in the relevant session; guests never create roster rows. Display names may repeat and do not determine uniqueness.

[0002](../migrations/0002_initial_games.sql) seeds only Mahjong and Poker with an idempotent insert. There is no roster or ledger seed. Indexes support owner/state draft lookup, roster entry lookup and deterministic session date order; reporting APIs remain planned.

Cents are stored as signed SQLite `INTEGER`. Bindings pass their canonical decimal strings through `CAST(? AS INTEGER)` and read `CAST(amount_cents AS TEXT)`, avoiding D1's JavaScript number precision limit. Numeric owners also use canonical decimal text. The integration tests verify `i64::MIN`, `i64::MAX` and a balancing cent without rounding.

## Atomic writes and replay

Draft replacement uses an optimistic revision check inside a D1 batch. An incorrect owner, revision or state forces a constraint failure before any participant replacement. Reading each header and its participants in one batch prevents mixed snapshots.

Confirmation first checks the durable update result, then verifies owner, open state and revision and invokes the pure zero-sum confirmation rules. A single D1 batch conditionally inserts the session for that exact draft revision, inserts every validated entry, consumes and increments the draft, and writes the successful update result. The session primary key permits exactly one session per draft. A failed conditional insert causes the mandatory entries to fail foreign-key or uniqueness checks; D1 rolls back the whole batch. This relies on D1's documented [transactional batch behavior](https://developers.cloudflare.com/d1/worker-api/d1-database/#batch).

The same update key returns its original session and creation instant, including after a lost response. A key reused for a different owner/draft is rejected. Another update against a consumed draft returns `DraftConsumed`. After a failed attempt, the use case rechecks the result before classifying draft conflicts, including when another request finishes between the initial result lookup and draft read. Failures never persist a success marker. Confirmed sessions expose no update/delete API; audited correction remains Phase 6.

## Local tests and limitations

`pnpm test:d1` runs the production Rust use cases and D1 adapter inside Wrangler's real local workerd runtime. Native tests cannot execute `worker::D1Database`, whose implementation calls JavaScript bindings. A separate `d1-test` feature therefore exports a test RPC function; [the small test-only JavaScript wrapper](../tests/worker.mjs) injects the Worker environment. The normal build excludes that export. There are no test HTTP endpoints, extra backend services or new JavaScript dependencies.

The harness uses Wrangler 4.150.0's `createTestHarness`, `getEnv`, `getExport` and `applyD1Migrations` APIs with ephemeral local storage. It verifies an empty schema before migrations and fails if unexpected application tables exist. Failure triggers inject errors in the real database at intermediate entry and final result writes, then tests check all rollback effects and safe retry. A separate native unit repository tests the deterministic lookup race; it is not counted as a D1 integration test.

The pinned workerd reports `2026-10-13` as its newest compatibility date and rejects the production `2026-10-14` date. Only [the test configuration](../tests/wrangler.toml) uses `2026-10-13`; the production configuration remains unchanged. These tests do not prove remote D1 behavior, deployment configuration or Telegram authorization. A successful remote CI run is still required to close Phase 2.

`pnpm db:migrate:local` targets only the `local` configuration and local D1 storage. Local placeholder database IDs are not staging/production resources. Remote migrations remain a separately authorized operation; Worker rollback does not restore schema or data.
