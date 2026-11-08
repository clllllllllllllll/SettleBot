# Backup and recovery

Phase 6 keeps confirmed sessions append-only. Corrections and voids add rows in
`session_audits` and `session_audit_entries`; they never update or delete the original
`sessions` or `session_entries` rows. A correction replaces only the effective amounts for
the existing ordered participants. A void removes the session from current histories and
aggregates while `/details` retains the original or last corrected values and full lineage.

## Portable ledger export

The private `GET /admin/export` route is the production export entry point. It requires the
`EXPORT_SECRET` Worker secret as an exact `Authorization: Bearer` value, returns `Cache-Control:
no-store`, and is separate from Telegram. Missing or incorrect credentials return HTTP 403.
Values shorter than 16 bytes fail closed. Configure a high-entropy secret independently in staging and production and never reuse the Telegram bot
or webhook tokens.

The route produces deterministic `settlebot-ledger-v1` JSON chunks. Start without a query and
follow `next_cursor` with `?cursor=...` until it is null. The cursor captures maximum player, game,
session, and audit row IDs, so later catalog or ledger appends do not enter the export. Independent player and game positions paginate every snapshotted catalog row, including inactive players, before session data. There is no total catalog, report-page, or 10,000-session
ceiling. Every chunk has fixed caps for catalogs, original entries, audit revisions, and audit
entries, and uses a fixed upper bound on D1 queries. Integer cents and numeric Telegram identities
are strings so JavaScript cannot round them.

The operator CLI follows the cursor and writes newline-delimited JSON to standard output:

```shell
SETTLEBOT_EXPORT_URL=https://settlebot.example/admin/export \
SETTLEBOT_EXPORT_TOKEN=replace-privately \
pnpm export:ledger > settlebot-ledger.ndjson
```

Keep the URL and token in the operator environment or an approved secret manager. Never commit
the token or output; the export contains private roster and ledger data. The local recovery
harness calls the same production route, while normal Worker builds expose no test endpoint.

## Authoritative D1 backup

Before a destructive migration or recovery, identify the exact environment and database.
Staging and production configuration do not exist yet, so the remote examples below are Phase 7
procedures, not commands that currently work. They require explicit authorization.

1. Stop or otherwise exclude writes for the recovery window.
2. Export the complete source database, including schema and operational tables:

   ```shell
   pnpm exec wrangler d1 export LEDGER_DB --remote --env production --output backups/settlebot-YYYYMMDDTHHMMSSZ.sql
   ```

3. Record a SHA-256 checksum and store the SQL file in encrypted storage with access logging.
   Never commit it; it contains private roster and ledger data.
4. Create a new isolated recovery D1 database. Never restore over the source database.
5. Restore the SQL export into that isolated target:

   ```shell
   pnpm exec wrangler d1 execute RECOVERY_DB --remote --file backups/settlebot-YYYYMMDDTHHMMSSZ.sql
   ```

6. Compare source and target counts for `players`, `games`, `sessions`, `session_entries`,
   `session_audits`, and `session_audit_entries`. Verify representative current leaderboards,
   a corrected session, a voided session, and their `/details` lineage. Keep the old database
   unchanged until the restored target passes verification.
7. Cut over only through a reviewed binding change and the Phase 7 staging and production
   approval gates. A Worker rollback does not restore D1 data or schema.

`pnpm test:d1` performs the local recovery exercise against two isolated workerd D1 databases.
It exports portable JSON from the source, restores original and audit rows into the target, and
compares record counts, effective ledger totals, and audit lineage. This is local evidence only;
no remote backup or restore has been performed.

## Migration procedure

- Back up and verify before applying a migration to a populated environment.
- Review migrations as forward-only, additive changes. Do not edit an applied migration.
- Apply migrations separately from Worker deployment with the exact environment selected:

  ```shell
  pnpm exec wrangler d1 migrations apply LEDGER_DB --remote --env staging
  ```

- Verify the migration table, health check, current reports, corrections, voids, and export in
  staging before production approval.
- If a migration fails, stop. Restore into a new isolated database from the verified backup;
  do not improvise destructive reverse SQL against the only copy.
