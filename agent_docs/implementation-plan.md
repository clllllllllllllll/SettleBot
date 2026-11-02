# Implementation plan

This is a plan, not a claim of implemented product capabilities. Read [README.md](../README.md) and [AGENTS.md](../AGENTS.md) first. Complete phases in order and record verification evidence before changing their status. External actions still require the authorization specified in AGENTS.md.

## Current status

Phases 0–5 are implemented and verified locally. Their implementation commits are on `main` and passed the validation-only [CI workflow](../.github/workflows/ci.yml), which runs formatting, linting, native tests, local D1/workerd integration tests, and a production Worker build without deployment. The [Worker](../src/lib.rs), platform-independent [domain module](../src/domain/mod.rs), [persistence module](../src/persistence/mod.rs), and [Telegram module](../src/telegram/mod.rs) have verified local behavior through Phase 5. This evidence does not verify remote D1, a deployed Worker, staging or production configuration, or live Telegram. The reporting batching remediation after the Phase 5 implementation commit is verified locally only and still requires a commit and successful CI run for remote validation. Phases 6–7 remain explicitly outstanding; the next authorized implementation gate is Phase 6 audit and recovery, not production delivery.

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

### Verification evidence

The [persistence module](../src/persistence/mod.rs), [migrations](../migrations/), and [local D1 integration tests](../tests/d1.test.mjs) implement this phase. The [persistence notes](persistence.md) document the atomic write strategy, stable identity policy, integer precision, and the verified need for a test-only Wasm/RPC bridge.

Local checks passed on 2026-10-20 with Rust 1.99.0/MSVC, Node.js 24.19.0, pnpm 11.25.0, and Wrangler 4.150.0:

- `pnpm install --frozen-lockfile`
- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 44 passed, including all 36 existing tests and 8 persistence unit tests.
- `pnpm test:d1` — 13 passed against real local D1/workerd, starting with an empty ephemeral database and applying both migrations. Covers seed idempotency, draft resumption/revisions and ownership, stable IDs, scoped guests, same/different-key concurrency, double confirmation, atomic rollback at entry and final-result writes, retry/lost-response replay, full signed cent boundaries, corrupt persisted values, and unrelated players.
- `pnpm db:migrate:local` — both migrations applied successfully to the local development database only.
- `pnpm build` — release WebAssembly build and deployment dry run succeeded with no production bindings. No deployment was performed.

Normal builds exclude the feature-gated test RPC bridge. The pinned local workerd rejects `2026-10-14` and reports `2026-10-13` as its newest date, so only the isolated test configuration uses `2026-10-13`. Production configuration is unchanged. Cached build tools ran in offline mode. No remote resources, migrations, secrets, webhooks, or deployments were used.

Remote CI passed for implementation commit [`8076d50f163cc39fb9dcb12549fdf836216818cc`](https://github.com/clllllllllllllll/SettleBot/commit/8076d50f163cc39fb9dcb12549fdf836216818cc) in [GitHub Actions run `39384311556`](https://github.com/clllllllllllllll/SettleBot/actions/runs/39384311556). The run completed formatting, Clippy, all native tests, real local D1 integration tests, and the production Worker build successfully, closing Phase 2.

## Phase 3 — Authorized Telegram workflow

- Validate the webhook secret before processing an update. Accept updates only from one configured numeric Telegram `chat.id`; reject private chats, other groups, channels, anonymous senders, and updates without a verifiable chat and user identity.
- Authorize state changes through an explicit allowlist of numeric Telegram user IDs, independent of Telegram group-admin status. Never authorize by username or display name. Apply the check before drafts, callbacks, roster creation, game creation, confirmation, correction, or voiding.
- Store the group ID, administrator IDs, credentials, and webhook secret outside tracked data with separate staging and production values. Verify every callback against its group, user, draft owner, and current draft state instead of trusting button payloads.
- Guide administrators through game selection, occurred-on date, roster selection or one-time guest entry, signed amounts, review, and confirmation. Support admin-controlled permanent roster and game creation. Guests are selectable only within their own draft/session and never appear in normal roster pickers.
- Gate: transport tests cover valid flows and reject missing/invalid secrets, private chats, other groups, channels, anonymous senders, non-allowlisted users including Telegram group administrators, forged/stale callbacks, and another user's draft without writes. Verify review totals/date, guest isolation, and safe replay behavior through persistence.

### Verification evidence

The [Telegram module](../src/telegram/mod.rs), [workflow migration](../migrations/0003_telegram_workflow.sql), and [Telegram integration tests](../tests/telegram.test.mjs) implement this gate. [Telegram notes](telegram.md) describe the commands, required secrets, authorization boundary, and delivery limitations. The original Phase 2 completion evidence is retained above.

Local checks passed on 2026-11-02 with the pinned toolchains:

- `pnpm install --frozen-lockfile`
- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 56 passed, including 12 new authorization, parsing, and HTTP-response tests.
- `pnpm test:d1` — the full suite builds the feature-gated Rust test Worker and applies all three migrations to empty local D1 databases. Final integration execution: 28 passed (13 persistence and 15 Telegram cases). Tests exercise the real HTTP adapter's rejection path, zero-write rejection, owner/revision/state guards, complete review/confirmation, roster-only pickers, guest isolation, concurrent same/different update IDs, rollback at the final receipt, retry after send failure, and full cent precision.
- `pnpm build` — release WebAssembly build and deployment dry run succeeded; no production bindings configured. Normal output excludes both test RPC exports.

Phase 3's gate is complete in implementation commit [`ef42b88a3ee5e18c6ffe3145df7a00bccc0162ea`](https://github.com/clllllllllllllll/SettleBot/commit/ef42b88a3ee5e18c6ffe3145df7a00bccc0162ea). Its formatting, Clippy, native tests, real local D1/workerd tests, and production Worker build passed in [GitHub Actions run `41462136749`](https://github.com/clllllllllllllll/SettleBot/actions/runs/41462136749). Outbound Telegram delivery used an injected transport; no live bot, webhook, remote D1 database, migration, or deployment was used.

## Phase 4 — Resilience

- Add explicit input sizes, participant/pagination limits, per-user rate limits, and abuse controls appropriate to low traffic. Bound external-call timeouts and retry only safe operations with limited attempts; never duplicate ledger writes.
- Return safe user-facing errors and structured logs without tokens, secrets, private roster details, or ledger amounts. Define handling for Telegram failures, D1 failures, and authorization lookup failures.
- Gate: tests exercise limits, malformed/oversized inputs, throttling, timeouts, retry exhaustion, redaction, and recovery after transient failures while preserving Phase 3 authorization and Phase 2 atomicity.

### Verification evidence

The [Telegram policy](../src/telegram/policy.rs), [rate-limit boundary](../src/telegram/rate.rs), [Worker adapter](../src/telegram/worker.rs), and [resilience migration](../migrations/0004_telegram_resilience.sql) implement this gate. Limits cover 16 KiB webhook bodies, 512-byte text commands, 64-byte callbacks, 64-character display names, 24 active players and games, 16 participants, and page sizes up to 50. Creation checks capacity before constructing a change, while D1 triggers preserve the catalog limit across concurrent requests. D1 atomically admits 20 new update IDs per administrator per rolling minute while preserving admission for duplicate delivery of an existing receipt-backed key after the original rate event expires.

Telegram fetch and response handling share a five-second deadline. Telegram sends and application/ledger writes are single-attempt operations; only idempotent rate admission retries, at most twice. Errors map to safe HTTP responses, recovery prompts never echo names or amounts, and structured logs contain only a static event name, category, and retryability.

Local checks passed on 2026-11-02 with the pinned toolchains:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 63 passed, including limit, timeout, retry recovery/exhaustion, and log-redaction tests.
- `pnpm test:d1` — 31 passed (13 persistence and 18 Telegram cases) after applying all four migrations to empty local D1 databases. New cases cover oversized inputs, catalog capacity and concurrent creation, atomic per-user throttling, delayed duplicate-key admission, rolling-window recovery, and timeout replay without duplicate ledger writes. Existing authorization, concurrency, and transaction rollback cases remain green.
- `pnpm build` — release WebAssembly build and Wrangler deployment dry run succeeded; no deployment was performed.

Phase 4's gate is complete in implementation commit [`f830bc4641958e22e59bea1993d3664d48a985e4`](https://github.com/clllllllllllllll/SettleBot/commit/f830bc4641958e22e59bea1993d3664d48a985e4). Its formatting, Clippy, native tests, real local D1/workerd tests, and production Worker build passed in [GitHub Actions run `41476300605`](https://github.com/clllllllllllllll/SettleBot/actions/runs/41476300605). No live Telegram call, remote D1 migration, secret change, cloud resource, or deployment was used.

## Phase 5 — Reporting

- Add roster-only overall and per-game leaderboards, player/session drill-downs, exact occurred-on dates, and bounded pagination. Show guest identities and entries only in the relevant detailed session history.
- Gate: query and presentation tests prove guests never enter roster rankings or ordinary pickers, unselected players are unchanged, per-game totals are correct, and pagination has deterministic ordering without missing or repeated records for an unchanged dataset.

### Verification evidence

The typed [reporting contract](../src/persistence/reporting.rs), [D1 queries](../src/persistence/d1/reporting.rs), and [Telegram reporting boundary](../src/telegram/reporting.rs) implement this phase over the existing immutable ledger schema. Overall and per-game reports start from the roster, so unselected players retain exact zero results; only session details select guest identities and entries. Pages have validated row and page-number bounds plus complete identity tie-breakers.

Local checks passed on 2026-11-03 with the pinned toolchains:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --all-targets` — 65 passed, including typed report-page boundary tests and all prior domain, persistence, Telegram, and request tests.
- `pnpm test:d1` — 34 passed (13 persistence, 18 Telegram workflow/resilience, and 3 isolated reporting cases) after applying all four migrations to empty local D1 databases. The reporting fixture verifies roster-only overall and per-game totals, widened aggregation across `[i64::MAX, 1, -i64::MAX]`, a zero-result unselected player, exact player/session dates, detail-only guest visibility, read-only Telegram presentation, and deterministic two-page traversal without gaps or repeats.
- `pnpm build` — release WebAssembly build and Wrangler deployment dry run succeeded; no deployment was performed.

Phase 5's gate is complete in implementation commit [`53e6deae16dd89caabdba95a1d23ca50def771c3`](https://github.com/clllllllllllllll/SettleBot/commit/53e6deae16dd89caabdba95a1d23ca50def771c3). Its formatting, Clippy, native tests, 34 real local D1/workerd tests, and production Worker build passed in [GitHub Actions run `41482729510`](https://github.com/clllllllllllllll/SettleBot/actions/runs/41482729510).

The 2026-11-03 reporting remediation bounds contribution reads to deterministic 100-row keyset batches and accumulates only exact checked `i128` totals in Worker memory. A session-row boundary captured with each report's roster or player page excludes later appends from subsequent batches, preserving one logical report snapshot under the current immutable ledger model. Focused local D1 coverage spans multiple internal batches, numeric boundaries, pagination, and an append after the captured boundary. Local remediation validation passes 65 native tests, 35 real local D1/workerd tests, and the production Worker dry run. The remediation working tree has no remote CI evidence yet. No live Telegram call, remote D1 migration, secret change, cloud resource, or deployment was used.

## Phase 6 — Audit and recovery

- Implement authorized corrections through audited revisions or voids; preserve original records. Add CSV or JSON export, backups, and documented recovery procedures.
- Gate: tests verify audit lineage and corrected aggregates without silently rewriting history. Restore a backup into an isolated test database and verify record counts, ledger totals, and audit history. Document the tested recovery and migration procedures.

This is the next authorized phase. It must define how correction/void records interact with the immutable reporting snapshot boundary before changing the schema or aggregation model. Completion requires tested audit lineage, export, isolated backup restore, recovery verification, and concise operational documentation; none exists yet.

## Phase 7 — Delivery and operations

- Keep pull requests validation-only with no deployment. On `main`, run validation before automatic staging deployment. Isolate staging and production Workers, D1 databases, secrets, and Telegram test contexts.
- Run staging smoke tests; require explicit manual approval before production promotion of that same commit SHA. Verify the promoted SHA has successful staging evidence and cannot silently resolve to a newer branch head.
- Control migrations separately with backward-compatible changes and a tested recovery path. Add monitoring, operational alerts, and Worker rollback procedures that explicitly do not restore D1 schema or data.
- Gate: after explicit authorization for external setup/deployment, verify a PR cannot deploy, a failed `main` validation cannot deploy, staging failure blocks promotion, and production requires approval for the smoke-tested SHA. Record deployment/smoke evidence and a tested rollback/recovery exercise without secrets or private data.

Phase 7 has not started. The current validation-only workflow, successful CI history, and local dry runs do not establish staging, production promotion, rollback, monitoring, remote migration safety, or production readiness.
