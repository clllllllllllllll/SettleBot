# AGENTS.md

## Project overview

SettleBot is a serverless Telegram ledger for zero-sum group game settlements. Its foundation uses Rust compiled to WebAssembly for Cloudflare Workers, Cloudflare D1, and GitHub Actions; product features have not been implemented.

## Repository structure

- `src/lib.rs` — Worker entry point and top-level request routing.
- `src/health.rs` — health-check response boundary.
- `src/tests.rs` — native request-level routing and response tests.
- `Cargo.toml` and `Cargo.lock` — Rust crate configuration and locked dependencies.
- `wrangler.toml` — local and deployment configuration for Cloudflare Workers.
- `package.json`, `pnpm-lock.yaml`, and `pnpm-workspace.yaml` — pinned Wrangler tooling and dependency-build policy.
- `.github/workflows/ci.yml` — formatting, linting, test, and Worker build checks.
- `agent_docs/implementation-plan.md` — authoritative phase status and testable completion gates.
- `README.md` — product scope, architecture, development, operations, and privacy constraints.

No D1 migrations exist yet; add them with persistence implementation.

## Authoritative implementation phases

Read [the implementation plan](agent_docs/implementation-plan.md) before implementation. Follow its phases in order, inspect the repository, verify recorded status against code and tests, and continue from the earliest incomplete gate. Do not skip, reorder, or expand a phase without explicit user approval. Mark a phase complete only after implementation, tests, and relevant documentation are verified.

- **Phase 0 — Foundation (remote CI pending):** Request-level behavior tests and local validation checks pass; completion awaits successful remote CI evidence for the implementation commit.
- **Phase 1 — Domain core:** Typed money, players, guests, games, participants, session states and dates; pure invariant tests.
- **Phase 2 — Persistence:** Versioned D1 migrations, persistent drafts, atomic session and idempotency writes, and integration tests.
- **Phase 3 — Authorized Telegram workflow:** Webhook secret, designated group, and admin checks before writes; guided sessions and admin roster/game creation.
- **Phase 4 — Resilience:** Input and rate limits, safe logs/errors, timeouts, retries, and abuse controls.
- **Phase 5 — Reporting:** Add roster-only overall leaderboards, per-game breakdowns, player and session drill-downs, exact dates, guest visibility in relevant session details, and pagination.
- **Phase 6 — Audit and recovery:** Add audited corrections and voids, CSV or JSON export, backups, recovery verification, and operational documentation.
- **Phase 7 — Delivery and operations:** PR validation only; validated `main` commits deploy automatically to isolated staging; smoke tests and manual approval gate promotion of the same commit to production.

## Build, test, and development commands

Install locked Wrangler dependencies:

```shell
pnpm install --frozen-lockfile
```

Run formatting, linting, tests, and the production Worker build:

```shell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
pnpm build
```

Start a local Worker development server:

```shell
pnpm dev
```

Rust 1.99.0, Node.js 24, pnpm 11.25.0, and the `wasm32-unknown-unknown` target are pinned by repository configuration. Windows development also requires Microsoft C++ Build Tools and the Windows SDK for host-side Rust build scripts. Do not run `pnpm deploy` without explicit authorization.

## Role and objective

Act as an expert software architect and elite engineer. Write production-grade code that maximizes readability, minimizes cyclomatic complexity, and remains adaptable to changing requirements.

## Code style and conventions

### Core architectural pillars

- **Keep it simple (KISS):** Prefer explicit, boring code over clever or deeply nested abstractions.
- **Single responsibility (SOLID):** Every function, type, and module must do exactly one thing. If a function exceeds 20 lines, break it down.
- **Composition over inheritance:** Build features by composing small, independent Rust types, traits, and modules rather than creating rigid hierarchies.

### Complexity constraints

- **Nesting limit:** Allow no more than two levels of nesting. Extract deeper logic into focused helper functions.
- **Early returns:** Use guard clauses to handle errors and edge cases immediately. Avoid wrapping function bodies in large conditional branches.
- **Pure functions:** Prefer deterministic functions whose outputs follow from their inputs. Minimize side effects and global state changes.

### Future-proofing and maintainability

- **Dependency injection:** Pass databases, APIs, clocks, and configuration into components. Never hardcode external connections inside business logic.
- **Interface-driven design:** Define explicit Rust structs, enums, traits, and validated boundary types for inputs and outputs.
- **Loose coupling:** Isolate core ledger rules from Cloudflare bindings, Telegram transport code, and D1-specific database access.
- Prefer explicit domain types for identifiers, signed cent amounts, roster status, and session state instead of passing primitive strings and integers throughout the application.
- Avoid unchecked arithmetic, floating-point money calculations, and `unwrap()` or `expect()` on production request paths.

### Implementation workflow

When writing code, follow these steps in order:

1. **Interfaces and types:** Define inputs, outputs, invariants, and type-safety constraints.
2. **Edge-case planning:** Identify failure points such as missing values, empty collections, invalid amounts, duplicate updates, and API or database failures.
3. **Skeleton layout:** Write function signatures and brief comments outlining the execution flow.
4. **Lean implementation:** Add the required logic with the lowest practical complexity.
5. **Refactor review:** Check the nesting limit, early returns, function length, error handling, and separation of side effects before finalizing.

### Naming and output rules

- **Self-documenting names:** Use descriptive variable, function, type, and module names; avoid unclear abbreviations.
- **Zero magic values:** Centralize configuration, constants, and environment bindings in typed structures or named constants.
- **No speculative features:** Do not build hypothetical capabilities. Keep current code easy to replace or remove instead of over-engineering it.
- Use Rust edition 2021 and the toolchain defined in `rust-toolchain.toml`.
- Run Rustfmt and treat all Clippy warnings as errors before finalizing Rust changes.
- For Markdown, use ATX headings, sentence-case section titles, hyphen bullets, and relative repository paths.
- Keep documentation concise and distinguish planned capabilities from implemented behaviour.
- Prefer links to stable repository files over duplicating implementation details in documentation.

## Domain invariants

- Display SGD values in dollars and cents; store monetary values internally as signed integer cents.
- A saved session must contain at least two participants and total exactly zero cents.
- Update only players selected for a session.
- Keep one-time guests in session accounting, but exclude them from roster leaderboards and normal participant pickers.
- Show one-time guests only when viewing the relevant detailed session history.
- Preserve corrections through audited revisions or voids; do not silently rewrite historical records.

## Agent guardrails and rules

- Never commit Telegram tokens, Cloudflare credentials, webhook secrets, production IDs, real ledger records, or private roster data.
- Use generic sample identities in tracked fixtures and documentation.
- Do not edit `.git/` internals or rewrite repository history.
- Do not push commits, deploy Workers, create cloud resources, configure Telegram webhooks, or apply remote database migrations without explicit user authorization.
- Do not claim a feature, control, environment, test, or deployment exists until it is implemented and verified.
- Preserve the target separation between local, staging, and production environments once configuration exists.
- Keep pull-request workflows deployment-free; they may run only validation checks.
- Deploy successful `main` builds to staging automatically only after that workflow is implemented and authorized.
- Require staging smoke tests and explicit manual approval before promoting the same verified commit to production.
- Keep staging and production Workers, D1 databases, secrets, and Telegram test contexts isolated.
- Treat D1 migrations as a separate controlled operation; Worker rollback does not restore database schema or data.
- Require server-side authorization for every state-changing Telegram action; hidden buttons are not authorization.
- Design writes to be transactional and webhook handling to be idempotent before production use.
- Keep the architecture proportional to the expected low traffic and Cloudflare free-tier constraints.
- Keep runtime dependencies compatible with the Cloudflare Workers WebAssembly target; do not assume native threads, sockets, filesystems, or OS services are available.
- Do not introduce JavaScript glue, another backend language, or a separate server unless the repository's verified Rust/Workers constraints require it and the architectural change is documented.
- Before destructive schema or data operations, identify the exact target and confirm a tested backup or recovery path.

## Progressive disclosure pointers

- Read `README.md` first for product requirements, architecture intent, operational goals, and privacy boundaries.
- Then read [`agent_docs/implementation-plan.md`](agent_docs/implementation-plan.md) for status and testable completion gates.
- TODO: Add focused documents such as `agent_docs/architecture.md`, `agent_docs/deployment.md`, and `agent_docs/recovery.md` only when verified implementation details exist.
- If nested components later require different instructions, add a scoped `AGENTS.md` in that component; the nearest file takes precedence.
