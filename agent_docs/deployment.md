# Delivery and operations

Phase 7 is in preparation. No remote resources, GitHub environments, credentials, alerts,
webhooks, migrations, or deployments have been configured or verified. The `release` job in
[CI](../.github/workflows/ci.yml) has a literal `false` condition. Keep it disabled until
external setup and deployment are explicitly authorized. Committing these files alone cannot
enable delivery.

## Prepared release path

1. Pull requests and `main` run formatting, Clippy, native tests, local D1/workerd tests,
   delivery-tool tests, and a production build dry run. PRs never enter release jobs.
2. Validation packages only the current bundled JavaScript and its imported Wasm module, then
   runs both generated environment configurations against isolated local D1. These packaged
   Worker tests reject missing schema and pass after local migrations. A manifest records the
   full SHA and file hashes. Successful `main` runs retain an immutable Actions artifact for
   14 days; private configuration and ledger data are never packaged.
3. Once enabled, [release.yml](../.github/workflows/release.yml) checks out the triggering SHA,
   downloads that run's artifact by ID, verifies its SHA and checksums, and deploys staging
   without rebuilding. One release concurrency group covers staging through production
   approval, so releases cannot overwrite each other's staging test context. GitHub can replace
   older pending runs; this is not a guarantee that every queued commit deploys.
4. Staging smoke checks require the expected `X-SettleBot-Commit` health header, reject
   unauthenticated export/webhook requests, and read one authenticated export chunk to exercise
   the D1 schema. Redirects, wrong environment URLs, stale releases, and timeouts fail closed.
   Response bodies, tokens, and private data are never logged or retained by smoke tooling.
5. Production depends on successful staging and references the protected `production` GitHub
   environment. Required reviewers must be configured externally; YAML alone does not create
   an approval rule. Both jobs refuse deployment if the GitHub API cannot verify required
   reviewers. Production uses the same artifact ID and SHA, followed by the same read-only smoke.

The smoke checks do not establish live Telegram delivery, complete export traversal, ledger
write behavior, backup restoration, or alert delivery. Those need the separately authorized
staging exercises below. A failed post-deployment smoke leaves the deployed version in place
and blocks further promotion; it does not automatically roll back.

## Environment setup after authorization

Create separate `settlebot-staging` and `settlebot-production` Workers and D1 databases. Use
separate Telegram bots, groups, numeric allowlists, webhook secrets, and export secrets.
Staging uses only dummy identities and records. Remote IDs must stay outside tracked files.

Create GitHub environments named `staging` and `production`, both restricted to deployments
from `main`. Configure production required reviewers and disable administrator bypass. Choose
self-review policy explicitly for the operator team. Confirm the repository's GitHub plan
supports required reviewers; if unavailable, leave delivery disabled. Protect `main` with
required validation and review, including changes to workflows and deployment tooling.

Store these GitHub environment secrets separately in each environment:

| Secret | Purpose |
|---|---|
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account for the target |
| `CLOUDFLARE_API_TOKEN` | Scoped Worker deployment credential; do not use a global API key |
| `STAGING_D1_ID`, `PRODUCTION_D1_ID` | Full, distinct database UUIDs for isolation validation |
| `WORKER_ORIGIN` | Exact `https://settlebot-ENV.SUBDOMAIN.workers.dev` origin |
| `EXPORT_SECRET` | Target Worker's export secret, used only for read-only smoke |

Use the same reviewed pair of D1 IDs in both GitHub environments. Check bindings against actual
resources during setup; distinct strings alone cannot prove that each resource is correctly
assigned. Scope tokens to the intended account and minimum available permissions. If Cloudflare
token scope cannot isolate individual Workers, document that limitation and consider separate
accounts. Do not grant delivery automation permission to perform migrations or provision secrets
unless those capabilities are separately reviewed and authorized.

Provision the six Worker secrets listed in [deployment.json](../config/deployment.json) directly
in each Worker. Telegram credentials are not consumed or provisioned by CI. Required secrets are
declared in the generated Wrangler config. Initial Worker/secret bootstrap is a separate operation;
do not remove required-secret checks to bypass an incomplete setup.

The [configuration generator](../scripts/prepare-deployment.mjs) combines the shared template,
environment secrets, and verified artifact into ignored `.wrangler/staging.json` or
`.wrangler/production.json`. It has no network calls, rejects missing/shared/placeholder database
IDs, and fixes Worker names by environment. There is no top-level remote database binding in
`wrangler.toml`. Generated configs contain private IDs; never commit or upload them.

Before enabling the source condition, verify the Actions token can read production protection
settings, all secrets exist, schema migrations have been applied separately, and the configured
origin belongs to the intended Worker. Removing `false` is a separately reviewed activation
change requiring explicit authorization. No release workflow has a manual or `workflow_run`
entry point that bypasses CI validation.

## Migrations, backups, and rollback

Deployment jobs contain no D1 migration, resource-creation, secret-provisioning, or Telegram
webhook setup command. Follow [recovery.md](recovery.md) before applying remote migrations.
Use the generated config for the exact target, for example after separate authorization:

```shell
pnpm exec wrangler d1 migrations apply LEDGER_DB --remote --config .wrangler/staging.json
```

Review additive migrations for compatibility with both the new Worker and the previous version.
Before changing a populated database, confirm an encrypted backup and tested isolated recovery
path. Record the current Worker version ID, release SHA, database identity, and schema version
in private operations records before deployment.

If a deployment fails smoke or exhibits errors:

1. Stop promotion and identify the affected environment and current/previous version IDs.
2. Confirm the previous Worker remains compatible with the current D1 schema and secret bindings.
3. After authorization, select the recorded version explicitly; never assume the default previous
   version is the desired recovery target:

   ```shell
   pnpm exec wrangler rollback VERSION_ID --config .wrangler/staging.json
   ```

4. Repeat smoke with the previous release's full SHA and verify staging Telegram behavior.
5. If data recovery is needed, use an isolated database restore and reviewed binding cutover.
   **Worker rollback does not restore D1 schema or data.**

Do not rerun an old release merely to perform a production rollback: that would also redeploy
staging. A rollback is a separately authorized operation with recorded evidence. Artifact
expiration fails deployment rather than silently rebuilding a different artifact.

## Monitoring and alerts

The prepared configuration enables application logs, disables automatic invocation logs and
traces, and disables preview URLs. Existing application failure events are static and redact
secrets, identities, and amounts. Confirm retention, access, and actual log contents in staging
before production. Health proves the Worker is responding; authenticated smoke also tests D1.

External monitoring and alerts are not configured. During authorized setup, assign an owner and
private notification destination, monitor health failures, Worker errors, static storage/Telegram
failure categories, D1 failures, and account usage against the free-tier budget. Exercise a test
alert and record delivery and response. Review logs for private-data leakage; do not send ledger
exports to monitoring tools. Enable GitHub Actions failure notifications for the operator.

## Remaining Phase 7 evidence

- Validate the prepared workflow and config commit in remote CI.
- Verify GitHub protection settings, Cloudflare bindings, and environment/Telegram isolation.
- Show PR and failed validation runs cannot deploy, staging failure blocks promotion, and
  production waits for approval tied to the exact smoke-tested SHA and artifact.
- Record successful staging deployment and read-only smoke, then an authorized live Telegram
  exercise using dummy data, including confirmation replay, correction, and void.
- Exercise Worker rollback, isolated remote backup/restore, and operational alert delivery.
- Promote only after explicit approval; record version IDs, SHA, workflow links, and smoke results
  without publishing credentials, private identifiers, or ledger contents.

## Local validation

On 2026-11-08, Rustfmt, Clippy with warnings denied, 69 native tests, 37 existing D1/workerd
tests, six delivery-tool tests, and two packaged-Worker smoke tests passed. Actionlint 1.7.12
validated both workflows. The production build and both generated environment dry runs passed
using fake database IDs. No external deployment or environment setup was performed.

To repeat the packaged-Worker tests after `pnpm build`, first ensure the ignored `.release`
directory is absent or empty (the packager refuses to overwrite existing files), then run:

```shell
node scripts/release-artifact.mjs package FULL_COMMIT_SHA
pnpm test:release
```

CI supplies its exact commit SHA automatically. Local tests use fake secrets and ephemeral
databases, and override only the compatibility date to the pinned workerd's supported date.

References: [GitHub environment protection](https://docs.github.com/en/actions/reference/workflows-and-actions/deployments-and-environments),
[Wrangler configuration](https://developers.cloudflare.com/workers/wrangler/configuration/), and
[Worker deployment and rollback commands](https://developers.cloudflare.com/workers/wrangler/commands/workers/).
