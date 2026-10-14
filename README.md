# SettleBot

A production-minded serverless application built to demonstrate cloud architecture, automated delivery, secure operations, and cost-conscious system design.

> **Status:** Planning
>
> **Stack:** TypeScript, Cloudflare Workers, Cloudflare D1
>
> **Target cost:** S$0/month within Cloudflare's free tier

## Overview

SettleBot records zero-sum win/loss sessions for a private Telegram group. It tracks cumulative and game-specific results while preserving exact session history and auditable corrections.

The ledger is intentionally small in scope so the project can focus on the complete cloud application lifecycle: architecture, provisioning, testing, deployment, monitoring, security, backup, and recovery.

## Cloud and DevOps focus

### Serverless architecture

- Event-driven processing through Telegram webhooks
- Cloudflare Workers for compute and Cloudflare D1 for persistence
- Services that scale to zero instead of an always-running server
- Architecture sized for a low-traffic workload and the free tier

### Continuous delivery

- GitHub-based source control and change review
- Automated formatting, linting, type checking, and testing
- Deployment gates that prevent failed builds from reaching production
- Versioned database migrations and controlled releases

### Environment and secret management

- Separate local, staging, and production environments
- Environment-specific Worker and D1 configuration
- Encrypted Cloudflare secrets for credentials and production identifiers
- No secrets or production data committed to the repository

### Operations and recovery

- Structured logs and application health checks
- Idempotent webhook processing and transactional writes
- Scheduled backups and portable data exports
- Deployment, rollback, and recovery runbooks
- Documented cost limits and migration options

## Product capabilities

- Record sessions involving any subset of roster players
- Add one-time guests or permanent roster players
- Require every session to total exactly `S$0.00`
- Track overall and game-specific standings
- Show dated player and session histories
- Correct or void sessions without deleting audit history
- Restrict changes to administrators
- Export the ledger as CSV or JSON

One-time guests are excluded from leaderboards but remain visible in the relevant session details.

## Architecture

```mermaid
flowchart LR
    DEV[Developer] --> GH[GitHub]
    GH -->|CI checks and deploy| CW[Cloudflare Worker]
    TG[Telegram Group] -->|HTTPS webhook| CW
    CW --> D1[(Cloudflare D1)]
    CRON[Scheduled Backup] --> CW
```

Changes pass through automated checks before deployment. Telegram sends updates to a secured Cloudflare Worker, which validates permissions, applies the ledger rules, and stores transactional records in D1.

## Operational controls

- SGD amounts displayed in dollars and cents, and stored internally as integer cents for accuracy
- Transactional session writes
- Duplicate webhook protection
- Group and administrator restrictions
- Per-user rate limiting
- Indexed and paginated queries
- Audited corrections
- Encrypted secrets
- Backup and recovery procedures

## Planned environments

| Environment | Purpose |
|---|---|
| Local | Development and testing |
| Staging | Telegram testing with dummy data |
| Production | Private group ledger |

## Privacy

The repository will not contain Telegram tokens, production IDs, real ledger records, or Cloudflare credentials.
