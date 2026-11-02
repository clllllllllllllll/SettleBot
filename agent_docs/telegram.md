# Telegram workflow

## Boundaries and configuration

The [Worker adapter](../src/telegram/worker.rs) exposes `POST /telegram/webhook`. `GET /health` is unchanged. Domain rules remain independent of Telegram and Workers. The application receives injected repository, clock, and transport interfaces; the production transport uses Telegram `sendMessage`. Callback acknowledgements use the webhook response's `answerCallbackQuery` method, following the [Telegram Bot API](https://core.telegram.org/bots/api).

Required bindings are `LEDGER_DB` plus these secrets:

- `TELEGRAM_WEBHOOK_SECRET`: the same secret supplied when configuring the webhook; validated before reading the request body.
- `TELEGRAM_CHAT_ID`: one negative numeric group/supergroup ID.
- `TELEGRAM_ADMIN_IDS`: comma-separated positive numeric SettleBot administrator IDs.
- `TELEGRAM_BOT_USERNAME`: the bot's username without the leading `@`; addressed commands must match it case-insensitively.
- `TELEGRAM_BOT_TOKEN`: credentials for outbound bot replies.

[.dev.vars.example](../.dev.vars.example) contains fake values only. Keep real local values in ignored `.dev.vars`; use independently provisioned secrets, bots/test contexts, and databases for staging and production. Only the local database placeholder exists in the repository configuration. All three migrations are required. No remote bindings, secrets, webhook, or environment have been provisioned or tested by this implementation.

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

Catalog display names may repeat; generated stable IDs distinguish roster entries. A draft snapshots its game and active-roster choices so button indices remain stable when another administrator creates catalog entries. New catalog entries appear in new drafts. Guests exist only within their own draft/session and never enter ordinary roster pickers. Reporting remains Phase 5.

## Persistence and delivery

The [D1 workflow adapter](../src/persistence/d1/telegram.rs) stores incomplete selections before they can satisfy the domain draft's required game/date. Each successful catalog or workflow mutation commits a receipt keyed as `telegram_UPDATE_ID` in the same transaction. Receipts bind the authenticated group, user, input kind, and action and retain the original reply. Reusing a key for a different action or owner fails. Concurrent duplicate deliveries return the original receipt; conflicting revisions cannot overwrite the winning change.

Confirmation atomically writes the validated ledger draft, session, entries, consumed draft and workflow, Phase 2 processed-update record, and Telegram receipt. An injected failure at the last receipt write is tested to roll back all preceding writes. Confirmed ledger records remain immutable.

Replies are sent after database commit. A failed Telegram send produces HTTP 503; a repeated webhook replays the stored reply without repeating ledger changes. Telegram message delivery is not exactly once: a lost response can produce a duplicate prompt or notification. Old prompts remain visible but their actions fail revision/state checks; `/resume` retrieves the current prompt. Authorized invalid/stale inputs receive generic guidance without writes. Missing or incorrect webhook secrets receive HTTP 403. Authenticated updates from rejected chats, senders, or unsupported update types receive HTTP 200 without processing so Telegram does not retry them. Malformed JSON/update fields are also acknowledged without processing. Storage/configuration failures return a generic HTTP 503 and do not expose error details.

## Verification and remaining work

[Native tests](../src/telegram/tests.rs) prove unauthorized requests cannot even access the repository. [Local D1 tests](../tests/telegram.test.mjs) compare application tables before and after rejected requests, exercise real HTTP rejection, complete guided flows, date/total review, guest isolation, inactive-player filtering, full cent precision, concurrent edits/confirmations, rollback, and retry after delivery failure. Outbound sends use an injected transport; real Telegram delivery and staging smoke tests remain unverified.

Phase 4 will add comprehensive input/participant limits, rate limits, external-call deadlines, and bounded retry policies. Phase 3 does not claim these controls. Deployment and remote migrations remain separately authorized operations.
