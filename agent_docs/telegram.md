# Telegram workflow

## Boundaries and configuration

The [Worker adapter](../src/telegram/worker.rs) exposes `POST /telegram/webhook`. `GET /health` is unchanged. Domain rules remain independent of Telegram and Workers. The application receives injected repository, clock, and transport interfaces; the production transport uses Telegram `sendMessage`. Callback acknowledgements use the webhook response's `answerCallbackQuery` method, following the [Telegram Bot API](https://core.telegram.org/bots/api).

Required bindings are `LEDGER_DB` plus these secrets:

- `TELEGRAM_WEBHOOK_SECRET`: the same secret supplied when configuring the webhook; validated before reading the request body.
- `TELEGRAM_CHAT_ID`: one negative numeric group/supergroup ID.
- `TELEGRAM_ADMIN_IDS`: comma-separated positive numeric SettleBot administrator IDs.
- `TELEGRAM_BOT_USERNAME`: the bot's username without the leading `@`; addressed commands must match it case-insensitively.
- `TELEGRAM_BOT_TOKEN`: credentials for outbound bot replies.

[.dev.vars.example](../.dev.vars.example) contains fake values only. Keep real local values in ignored `.dev.vars`; use independently provisioned secrets, bots/test contexts, and databases for staging and production. Only the local database placeholder exists in the repository configuration. All four migrations are required. No remote bindings, secrets, webhook, or environment have been provisioned or tested by this implementation.

The authorization boundary rejects private chats, other groups, channels, anonymous/bot senders, business-account messages, inaccessible callback messages, and missing identities. It uses numeric IDs only; Telegram administrator status, usernames, and display names grant no permissions. Every mutation and replay checks the current allowlist. Every draft action also checks owner, group, revision, and workflow stage. Callback contents identify an action; they do not confer authority.

## Administrator commands

- Telegram's optional bot-address suffix is accepted for every command when it matches `TELEGRAM_BOT_USERNAME`, for example `/session@SettleBot` and `/player@SettleBot Sample Player`. Commands addressed to another bot are acknowledged without processing.
- `/player Sample Player` creates an active permanent roster player.
- `/game Sample Game` creates a custom game; Mahjong and Poker remain the initial seeds.
- `/session` starts a draft and offers game buttons.
- The date prompt supplies `/step SESSION_ID REVISION YYYY-MM-DD`. Enter the date played in Asia/Singapore; it is never inferred from the UTC creation instant.
- Select roster buttons, tapping again to remove. The prompt supplies `/guest SESSION_ID REVISION Guest Name` for each one-time guest. Choose “Enter amounts” after selecting at least two participants.
- For each selected participant, the prompt supplies `/step SESSION_ID REVISION +0.00`. Replace the amount with signed SGD using exactly two decimals, such as `+12.50` or `-12.50`.
- Review shows the game, exact played date, selected identities, guest labels, amounts, and total. Confirmation is available only at `S$0.00`; the server independently enforces all domain invariants. “Re-enter amounts” starts amount entry again.
- `/resume SESSION_ID` restores the current prompt for the draft owner. Start a new `/session` to replace a mistaken game/date/guest selection; historical sessions cannot be edited through this workflow.

Catalog display names may repeat; generated stable IDs distinguish roster entries. Creation stops at 24 active players or 24 games so every accepted entry remains available to new workflow pickers. The application checks capacity before creating the change, and D1 triggers enforce the same limit if requests race. A draft snapshots its game and active-roster choices so button indices remain stable when another administrator creates catalog entries. New catalog entries appear in new drafts. Guests exist only within their own draft/session and never enter ordinary roster pickers.

## Reporting commands

- `/leaderboard [PAGE]` lists the overall permanent-roster ranking.
- `/gameboard GAME_ID [PAGE]` lists the permanent-roster ranking for one game.
- `/stats PLAYER_ID [PAGE]` shows one roster player's overall total and dated session results.
- `/history [PAGE]` lists sessions by exact played date and stable session ID.
- `/details SESSION_ID` shows one session's game, exact played date, roster entries, and session-scoped guest identities and entries.

Pages default to 1 and contain 10 rows. Page sizes are validated at 1–50 rows inside the reporting boundary, and page numbers are capped at 10,000. Rankings use deterministic total/name/ID ordering; histories use played-date/session-ID ordering. Navigation text supplies the previous or next command when applicable.

All reporting commands pass through the same secret, numeric group, and numeric administrator authorization boundary as mutations. They are read-only and create no Telegram workflow receipt. Leaderboards include permanent roster players only, including roster players with a zero result. Player and ordinary session histories never resolve guest names or amounts; `/details` exposes guests only for the requested session.

## Persistence and delivery

The [D1 workflow adapter](../src/persistence/d1/telegram.rs) stores incomplete selections before they can satisfy the domain draft's required game/date. Each successful catalog or workflow mutation commits a receipt keyed as `telegram_UPDATE_ID` in the same transaction. Receipts bind the authenticated group, user, input kind, and action and retain the original reply. Reusing a key for a different action or owner fails. Concurrent duplicate deliveries return the original receipt; conflicting revisions cannot overwrite the winning change.

Confirmation atomically writes the validated ledger draft, session, entries, consumed draft and workflow, Phase 2 processed-update record, and Telegram receipt. An injected failure at the last receipt write is tested to roll back all preceding writes. Confirmed ledger records remain immutable.

Replies are sent after database commit. A failed Telegram send produces HTTP 503; a repeated webhook replays the stored reply without repeating ledger changes. Telegram message delivery is not exactly once: a lost response can produce a duplicate prompt or notification. Old prompts remain visible but their actions fail revision/state checks; `/resume` retrieves the current prompt. Authorized invalid/stale inputs receive generic guidance without workflow or ledger writes. Missing or incorrect webhook secrets receive HTTP 403. Authenticated updates from rejected chats, senders, or unsupported update types receive HTTP 200 without processing so Telegram does not retry them. Malformed JSON/update fields are also acknowledged without processing. Storage/configuration failures return a generic HTTP 503 and do not expose error details.

## Resilience and failure handling

The Worker rejects webhook bodies over 16 KiB before JSON decoding. Text commands are limited to 512 bytes, callback data to 64 bytes, callback IDs to 128 bytes, and display names to 64 characters. New workflows snapshot at most 24 games and 24 active players, and a session accepts at most 16 participants. Reporting pages contain 10 rows; the validated reporting types permit 1–50 rows and pages 1–10,000.

[0004](../migrations/0004_telegram_resilience.sql) records verified update keys for a rolling per-administrator limit of 20 new updates per minute. Admission is atomic in D1. Expired events without receipts are pruned; events backed by durable Telegram receipts remain so a delayed replay stays eligible without increasing the current-window count. A new key over the limit is acknowledged without processing or an outbound reply. Secret, numeric chat/user authorization, and command-address validation run before rate admission.

The complete outbound Telegram request and response body have one five-second deadline. Telegram transport failures, timeouts, D1 failures, and missing/invalid configuration return the same generic HTTP 503 body. Authorization failures never disclose which check failed; missing/incorrect webhook secrets return generic 403, and permanent authenticated rejections return HTTP 200 to stop retries.

Telegram sends are never retried inline because the API may have accepted a request before the response was lost. Workflow and ledger writes are also never retried outside their existing update-key replay transaction. Only rate admission retries a transient D1 failure, with two attempts, because its update-key operation is idempotent. Static JSON log events record only an event name, category, and retryability; they never include configuration values, IDs, submitted text, display names, or amounts.

## Verification and remaining work

[Native tests](../src/telegram/tests.rs) prove unauthorized requests cannot access the repository and cover limits, report parsing, retry exhaustion/recovery, deadlines, and log redaction. [Local D1 tests](../tests/telegram.test.mjs) compare application tables before and after rejected requests, exercise real HTTP rejection and oversized bodies, throttle by administrator/update key, complete guided flows, preserve full cent precision, race edits/confirmations, inject transaction failures, and replay safely after transport failure or timeout. [Reporting tests](../tests/reporting.test.mjs) verify roster-only totals, exact dates, stable pagination, read-only presentation, and session-local guest visibility. Outbound sends use an injected transport; real Telegram delivery and staging smoke tests remain unverified.

Phase 5's implementation commit passed the validation-only GitHub Actions workflow; the current reporting batching remediation is locally validated only. Deployment, live Telegram behavior, remote D1 and migrations, and staging operations remain unverified and separately authorized.
