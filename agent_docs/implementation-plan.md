# Implementation plan

This is a plan, not a claim of implemented product capabilities. Read [README.md](../README.md) and [AGENTS.md](../AGENTS.md) first. Complete phases in order and record verification evidence before changing their status. External actions still require the authorization specified in AGENTS.md.

## Current status

Phases 0 and 1 are complete. The [Worker](../src/lib.rs) has verified request-level health behavior, and the platform-independent [domain module](../src/domain/mod.rs) has verified ledger types and invariants. The [CI workflow](../.github/workflows/ci.yml) runs formatting, linting, tests, and build checks for pull requests and pushes to `main`, with no deployment. Phase 2 is next; Phases 2–7 remain unimplemented and have not started.

## Phase 0 — Foundation

- Keep pinned toolchains and the Rust/Workers build reproducible with locked dependencies.
- Automated request-level checks cover `GET /health`: HTTP 200, JSON content type, and `{"status":"ok"}`; unknown paths and unsupported methods return 404.
- Gate: formatting, Clippy with warnings denied, tests, and the production Worker build pass. The automated health checks run in CI. Record a successful GitHub Actions run URL and its implementation commit SHA; a workflow file or zero-test success alone is insufficient evidence.

### Verification evidence

Local checks passed on 2026-10-20 with the pinned toolchains:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 3 passed, 0 failed.
- `pnpm build` — release Worker build and Wrangler deployment dry run succeeded, with no configured bindings. No deployment was performed.

Remote CI passed for implementation commit [`71ec5910c1eadffdfdddad07e53037bdd3c6495f`](https://github.com/clllllllllllllll/SettleBot/commit/71ec5910c1eadffdfdddad07e53037bdd3c6495f) in [GitHub Actions run `39358093295`](https://github.com/clllllllllllllll/SettleBot/actions/runs/39358093295). The run completed formatting, Clippy, tests, and the production Worker build successfully.

## Phase 1 — Domain core

- Define validated identifiers and types for signed integer cents, roster players, session-scoped guests, games, participants, and draft/confirmed session state. Keep this layer independent of Workers, Telegram, and D1.
- Define `occurred_on` as the calendar date the game was played in Asia/Singapore, selected or confirmed by the administrator; it may differ from entry day. Define `created_at` as the UTC instant the confirmed record is created, supplied through an injected clock. Do not infer the played date from creation time.
- A confirmed session has one game, at least two distinct participants, exactly one amount per selected participant, and a checked total of zero cents. Unselected players cannot receive entries. Guests count toward the total but never become roster members implicitly.
- Gate: pure unit tests cover invalid/empty identifiers and required values, invalid dates, duplicate participants, missing/extra amounts, fewer than two participants, nonzero totals, numeric boundaries and overflow, SGD formatting, and roster/guest separation. Cover creation instants around Singapore midnight without changing the explicitly selected played date.

### Verification evidence

The [domain module](../src/domain/mod.rs) implements validated identities and values, separate draft and immutable confirmed types, session-scoped guests, and checked confirmation. [Pure tests](../src/domain/tests/mod.rs) cover the Phase 1 gate. Creation instants are supplied by the caller without a clock read; the explicitly selected played date is preserved across Singapore midnight.

Local checks passed on 2026-10-20 with the pinned toolchains and the installed Windows C++ build environment:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 36 passed, 0 failed: 33 domain tests and 3 request tests.
- `pnpm build` — release WebAssembly Worker build and Wrangler deployment dry run succeeded, with no configured bindings. Cached build tools ran in offline mode. No deployment was performed.

Chrono was already present in the lockfile; its explicit domain dependency disables default features and adds no packages. There is no persistence or Telegram integration in the domain.

Remote CI passed for implementation commit [`1b3141a65d971cb502964e89c58fb8ea8cb224e0`](https://github.com/clllllllllllllll/SettleBot/commit/1b3141a65d971cb502964e89c58fb8ea8cb224e0) in [GitHub Actions run `39360377728`](https://github.com/clllllllllllllll/SettleBot/actions/runs/39360377728). The run completed formatting, Clippy, all 36 tests, and the production Worker build successfully.

## Phase 2 — Persistence

- Add versioned D1 migrations and injected repository interfaces for roster, games, persistent drafts, confirmed sessions, entries, and processed updates. Seed Mahjong and Poker only; keep real roster names and ledger records out of tracked data.
- Persist draft ownership, selection, date, amounts, and revision/state so a later Worker request can resume the flow. Enforce uniqueness by stable identity rather than display name; document any display-name policy explicitly.
- Commit confirmation, all participant entries, draft consumption, and the successful idempotency result atomically. Distinguish replay of one update from a second confirmation update targeting the same draft. A failed transaction must not leave a success marker or partial ledger changes.
- Gate: local D1 integration tests apply migrations from an empty database and verify draft resumption, unique participants, valid guest accounting, duplicate-update replay, double confirmation, concurrent confirmations, injected write failure and rollback, and retry after failure or lost response. Each successful draft produces exactly one session; unrelated players remain unchanged.

## Phase 3 — Authorized Telegram workflow

- Validate the webhook secret before processing an update. Enforce the designated group and server-side administrator authorization before every state change, including drafts, callbacks, roster creation, and game creation; fail closed if authorization cannot be established.
- Store credentials and group configuration outside tracked data. Verify callback ownership and current draft state instead of trusting button payloads.
- Guide administrators through game selection, occurred-on date, roster selection or one-time guest entry, signed amounts, review, and confirmation. Support admin-controlled permanent roster and game creation. Guests are selectable only within their own draft/session and never appear in normal roster pickers.
- Gate: transport tests cover valid flows and reject missing/invalid secrets, other groups, non-admins, forged/stale callbacks, and another user's draft without writes. Verify review totals/date, guest isolation, and safe replay behavior through persistence.

## Phase 4 — Resilience

- Add explicit input sizes, participant/pagination limits, per-user rate limits, and abuse controls appropriate to low traffic. Bound external-call timeouts and retry only safe operations with limited attempts; never duplicate ledger writes.
- Return safe user-facing errors and structured logs without tokens, secrets, private roster details, or ledger amounts. Define handling for Telegram failures, D1 failures, and authorization lookup failures.
- Gate: tests exercise limits, malformed/oversized inputs, throttling, timeouts, retry exhaustion, redaction, and recovery after transient failures while preserving Phase 3 authorization and Phase 2 atomicity.

## Phase 5 — Reporting

- Add roster-only overall and per-game leaderboards, player/session drill-downs, exact occurred-on dates, and bounded pagination. Show guest identities and entries only in the relevant detailed session history.
- Gate: query and presentation tests prove guests never enter roster rankings or ordinary pickers, unselected players are unchanged, per-game totals are correct, and pagination has deterministic ordering without missing or repeated records for an unchanged dataset.

## Phase 6 — Audit and recovery

- Implement authorized corrections through audited revisions or voids; preserve original records. Add CSV or JSON export, backups, and documented recovery procedures.
- Gate: tests verify audit lineage and corrected aggregates without silently rewriting history. Restore a backup into an isolated test database and verify record counts, ledger totals, and audit history. Document the tested recovery and migration procedures.

## Phase 7 — Delivery and operations

- Keep pull requests validation-only with no deployment. On `main`, run validation before automatic staging deployment. Isolate staging and production Workers, D1 databases, secrets, and Telegram test contexts.
- Run staging smoke tests; require explicit manual approval before production promotion of that same commit SHA. Verify the promoted SHA has successful staging evidence and cannot silently resolve to a newer branch head.
- Control migrations separately with backward-compatible changes and a tested recovery path. Add monitoring, operational alerts, and Worker rollback procedures that explicitly do not restore D1 schema or data.
- Gate: after explicit authorization for external setup/deployment, verify a PR cannot deploy, a failed `main` validation cannot deploy, staging failure blocks promotion, and production requires approval for the smoke-tested SHA. Record deployment/smoke evidence and a tested rollback/recovery exercise without secrets or private data.
