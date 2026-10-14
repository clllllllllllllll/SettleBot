# AGENTS.md

## Project Overview

SettleBot is a planned serverless Telegram ledger for zero-sum group game settlements. The target stack is Rust compiled to WebAssembly for Cloudflare Workers, Cloudflare D1, and GitHub-based CI/CD; implementation has not started.

## Repository Structure

- `README.md` — product scope, target architecture, operational controls, and privacy constraints.
- `AGENTS.md` — repository-specific instructions for coding agents.
- `agent_docs/` — not present. Add focused documentation here only when details outgrow this file.

No source, test, migration, infrastructure, container, or workflow directories exist yet.

## Role & Objective

Act as an expert software architect and elite engineer. Write production-grade code that maximizes readability, minimizes cyclomatic complexity, and remains adaptable to changing requirements.

## Code Style & Conventions

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
- For Markdown, use ATX headings, sentence-case section titles, hyphen bullets, and relative repository paths.
- Keep documentation concise and distinguish planned capabilities from implemented behaviour.
- Prefer links to stable repository files over duplicating implementation details in documentation.
- TODO: Document Rust edition, minimum supported Rust version, formatting, linting, error-handling, dependency, and test conventions after the relevant tools are configured.

## Domain Invariants

- Display SGD values in dollars and cents; store monetary values internally as signed integer cents.
- A saved session must contain at least two participants and total exactly zero cents.
- Update only players selected for a session.
- Keep one-time guests in session accounting, but exclude them from roster leaderboards and normal participant pickers.
- Show one-time guests only when viewing the relevant detailed session history.
- Preserve corrections through audited revisions or voids; do not silently rewrite historical records.

## Agent Guardrails & Rules

- Never commit Telegram tokens, Cloudflare credentials, webhook secrets, production IDs, real ledger records, or private roster data.
- Use generic sample identities in tracked fixtures and documentation.
- Do not edit `.git/` internals or rewrite repository history.
- Do not push commits, deploy Workers, create cloud resources, configure Telegram webhooks, or apply remote database migrations without explicit user authorization.
- Do not claim a feature, control, environment, test, or deployment exists until it is implemented and verified.
- Preserve the target separation between local, staging, and production environments once configuration exists.
- Require server-side authorization for every state-changing Telegram action; hidden buttons are not authorization.
- Design writes to be transactional and webhook handling to be idempotent before production use.
- Keep the architecture proportional to the expected low traffic and Cloudflare free-tier constraints.
- Keep runtime dependencies compatible with the Cloudflare Workers WebAssembly target; do not assume native threads, sockets, filesystems, or OS services are available.
- Do not introduce JavaScript glue, another backend language, or a separate server unless the repository's verified Rust/Workers constraints require it and the architectural change is documented.
- Before destructive schema or data operations, identify the exact target and confirm a tested backup or recovery path.

## Progressive Disclosure Pointers

- Read `README.md` first for product requirements, architecture intent, operational goals, and privacy boundaries.
- No `agent_docs/` directory exists yet.
- TODO: Add focused documents such as `agent_docs/architecture.md`, `agent_docs/deployment.md`, and `agent_docs/recovery.md` only when verified implementation details exist.
- If nested components later require different instructions, add a scoped `AGENTS.md` in that component; the nearest file takes precedence.
