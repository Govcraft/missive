<p align="center">
  <img src="branding/missive_logomark.png" alt="Missive" width="128" height="128">
</p>

<h3 align="center">Self-hostable mail, calendar, and contacts</h3>

<p align="center">
  A personal information manager built with Rust that ships as a single binary.<br>
  Works with any JMAP-compliant mail server. No JavaScript framework required.
</p>

<p align="center">
  <a href="#license"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License: AGPL-3.0"></a>
  <img src="https://img.shields.io/badge/rust-2024_edition-orange" alt="Rust 2024 Edition">
  <img src="https://img.shields.io/badge/JMAP-RFC_8620-green" alt="JMAP RFC 8620">
</p>

---

## Overview

Missive is a self-hostable PIM suite -- mail, calendar, and contacts -- that runs as a single binary with all assets embedded. It communicates with your mail server over [JMAP](https://jmap.io/) (RFC 8620), the modern successor to IMAP, and renders a responsive UI entirely server-side with HTMX. The result is a fast, lightweight frontend with no client-side JavaScript framework, no build-time asset pipeline to maintain, and no external file tree to deploy.

Missive is built on [acton-service](https://govcraft.github.io/acton-service/), a production-ready Rust backend framework that provides type-enforced API versioning, automatic health/readiness endpoints, structured logging, session management, and SSE broadcasting out of the box. Built and tested against [Stalwart Mail Server](https://stalw.art/), Missive works with any JMAP-compliant server.

## Features

Missive provides a complete webmail experience with calendar and contacts, installable as a progressive web app on desktop and mobile.

### Mail

- **Three-pane layout** -- mailbox sidebar, message list, and reading pane in a single view
- **Compose, reply, forward** -- rich text editor (Trix) with file attachments and CC/BCC
- **Full-text search** -- search across mailbox contents via JMAP query
- **Bulk actions** -- multi-select messages for delete, move, archive, spam, or read/unread toggling
- **Star and flag** -- mark important messages with `$flagged` keyword support
- **Move between folders** -- organize messages across mailboxes
- **HTML sanitization** -- safe rendering of HTML emails via Ammonia-based filtering

### Calendar and Contacts

- **Calendar view** -- browse and manage calendar events
- **Contact management** -- view and organize contacts
- **Unified navigation** -- switch between Mail, Calendar, and Contacts from the sidebar

### Experience

- **Progressive web app** -- installable on desktop and mobile for an app-like experience
- **Mobile responsive** -- fully functional on phones and tablets
- **Dark mode** -- system-aware theme with manual toggle
- **Keyboard shortcuts** -- navigate, compose, reply, delete, and archive without a mouse
- **Browser notifications** -- opt-in push notifications for new mail arrivals

### Operations

- **Single binary** -- all static assets embedded via `rust-embed`; nothing to deploy but the executable
- **Session backends** -- in-memory sessions for development, Redis for production persistence
- **Health checks** -- automatic `/health` (liveness) and `/ready` (readiness) endpoints
- **CLI tooling** -- interactive setup wizard (`missive setup`), configuration validator (`missive sanity`), and config generator (`missive config`)
- **Structured logging** -- JSON-formatted logs with optional systemd journald integration

### Integrations

- **Real-time updates** -- server-sent events (SSE) push new mail and mailbox changes to the browser instantly
- **Webhook delivery** -- HTTP POST notifications for email lifecycle events with optional HMAC-SHA256 signing
- **JMAP standard** -- works with any compliant server, not tied to a single vendor

## Screenshots

> Screenshots coming soon. Missive features a three-pane interface with dark mode support, mobile-responsive layout, and an installable PWA experience.

## Getting Started

Missive requires a running JMAP-compliant mail server such as [Stalwart](https://stalw.art/). For building from source, you need Rust 1.85+ (2024 edition), Node.js 22+, and [pnpm](https://pnpm.io/).

### Quick Start: Binary

Build and run Missive directly on your machine.

```bash
# Clone and build
git clone https://github.com/Govcraft/missive.git
cd missive
pnpm install && pnpm run build
cargo build --release

# Generate a starter config
./target/release/missive config --output config.toml
# Edit config.toml to set your jmap_url

# Or run the interactive setup wizard
./target/release/missive setup

# Start the server
./target/release/missive
```

Open `http://localhost:8080` and log in with your mail server credentials.

### Quick Start: Docker

```bash
docker build -t missive .
docker run -p 8080:8080 \
  -e ACTON_JMAP_URL=https://your-mail-server.example.com \
  missive
```

The Docker image uses a multi-stage build that produces a minimal Debian-based runtime image. A built-in health check pings `/health` every 30 seconds.

## Configuration

Missive uses a layered configuration system powered by acton-service's [Figment-based config](https://govcraft.github.io/acton-service/docs/configuration). Environment variables take precedence over config files and use the `ACTON_` prefix.

### Config File

```toml
jmap_url = "https://mail.example.com"
page_size = 50

[service]
name = "missive"
port = 8080

[session]
storage = "memory"                        # "memory" or "redis"
redis_url = "redis://localhost:6379"       # required when storage = "redis"
```

Place the file in any of these locations (highest priority first):

1. `./config.toml` (working directory)
2. `~/.config/acton-service/missive/config.toml`
3. `/etc/acton-service/missive/config.toml`

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ACTON_JMAP_URL` | Base URL of your JMAP mail server | *(required)* |
| `ACTON_PAGE_SIZE` | Number of emails per page | `50` |
| `ACTON_SERVICE_PORT` | HTTP listen port | `8080` |
| `ACTON_SERVICE_NAME` | Service name for logging | `missive` |
| `ACTON_SESSION_STORAGE` | Session backend: `memory` or `redis` | `memory` |
| `ACTON_SESSION_REDIS_URL` | Redis connection string | *(required if redis)* |

### Session Backends

**In-memory** sessions work out of the box for development and single-instance deployments. Sessions are lost on restart.

**Redis** sessions persist across restarts and support multi-instance deployments behind a load balancer. Enable by setting `session.storage = "redis"` and providing a `redis_url`.

## Deployment

Missive supports three deployment models: Docker Compose with automatic HTTPS, systemd for bare-metal servers, and manual reverse proxy configurations.

### Docker Compose with Traefik

The included `docker-compose.yml` sets up Missive behind Traefik with automatic HTTPS via Let's Encrypt.

```bash
# Create a .env file
cat > .env << 'EOF'
DOMAIN=mail.example.com
ACME_EMAIL=admin@example.com
ACTON_JMAP_URL=https://your-mail-server.example.com
EOF

# Start the stack
docker compose up -d
```

To enable Redis-backed sessions, add these to your `.env` and activate the Redis profile:

```bash
echo 'ACTON_SESSION_STORAGE=redis' >> .env
echo 'ACTON_SESSION_REDIS_URL=redis://redis:6379' >> .env

docker compose --profile redis up -d
```

### Systemd Service

A hardened unit file is provided at `deploy/missive.service` with `ProtectSystem=strict`, `ProtectHome=true`, and `NoNewPrivileges=true`.

```bash
# Install the binary
sudo cp target/release/missive /opt/missive/
sudo cp deploy/missive.service /etc/systemd/system/

# Create the service user and environment file
sudo useradd --system --create-home missive
sudo tee /opt/missive/.env << 'EOF'
ACTON_JMAP_URL=https://your-mail-server.example.com
ACTON_SERVICE_PORT=8080
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable --now missive
```

### Reverse Proxy

Missive binds to `0.0.0.0:8080` by default. When running behind nginx, Caddy, or Traefik, forward traffic to that port and configure for SSE:

- **Disable response buffering** for `/api/v1/events` so server-sent events stream correctly
- **Extend read timeouts** -- SSE connections are long-lived
- **WebSocket is not required** -- Missive uses SSE, not WebSockets

Use `/health` for liveness probes and `/ready` for readiness checks.

## Webhooks

Missive can deliver HTTP POST notifications for email lifecycle events, enabling integrations without polling. A background worker monitors JMAP state changes and posts structured JSON payloads to your configured endpoint.

### Webhook Configuration

```toml
[webhook]
url = "https://your-app.example.com/webhook"
secret = "your-hmac-secret"
jmap_username = "user@example.com"
jmap_password = "password"
include_body = false
ping_interval = 60
```

| Field | Description | Default |
|-------|-------------|---------|
| `url` | Endpoint to receive webhook POST requests | *(required)* |
| `secret` | HMAC-SHA256 signing key (omit to disable signing) | *(none)* |
| `jmap_username` | JMAP account for monitoring state changes | *(required)* |
| `jmap_password` | Password for the JMAP account | *(required)* |
| `include_body` | Include email body text in payloads | `false` |
| `ping_interval` | JMAP EventSource ping interval in seconds | `60` |

### Event Types

| Event | Trigger |
|-------|---------|
| `email.received` | New email arrives |
| `email.updated` | Email flags or mailbox assignment changes |
| `email.deleted` | Email permanently deleted |

### Payload Format

Payloads for `email.received` and `email.updated` events:

```json
{
  "event": "email.received",
  "email_id": "M1234",
  "message_id": ["<abc@example.com>"],
  "thread_id": "T5678",
  "mailbox_ids": ["inbox-id"],
  "subject": "Hello",
  "from": [{"name": "Alice", "email": "alice@example.com"}],
  "to": [{"name": "Bob", "email": "bob@example.com"}],
  "cc": [],
  "reply_to": [],
  "in_reply_to": [],
  "references": [],
  "preview": "First 256 characters...",
  "body_text": null,
  "has_attachment": false,
  "sent_at": 1710000000,
  "received_at": 1710000001,
  "keywords": ["$seen"],
  "size": 4096
}
```

Payloads for `email.deleted` events contain only `event` and `email_id`.

### HMAC Signing

When a `secret` is configured, every POST includes an `X-Signature` header:

```
X-Signature: sha256=734cc62f32841568f45715aeb9f4d7891324e6d948e4c6c60c0621cdac48623a
```

Verify by computing HMAC-SHA256 of the raw request body with your secret and comparing to the hex digest after the `sha256=` prefix.

## Architecture

Missive acts as a thin translation layer between HTMX in the browser and JMAP on the server. There is no intermediate database -- all state lives on your mail server. Every request flows through the same pipeline:

```
Browser                   Missive                        Mail Server
  |                         |                                |
  |  HTTP/HTMX request      |                                |
  |------------------------>|                                |
  |                         |  AuthenticatedClient           |
  |                         |  extractor validates session,  |
  |                         |  retrieves cached JMAP client  |
  |                         |                                |
  |                         |  JMAP request (RFC 8620)       |
  |                         |------------------------------->|
  |                         |                                |
  |                         |  JMAP response                 |
  |                         |<-------------------------------|
  |                         |                                |
  |  HTML partial (HTMX)   |  Askama template renders       |
  |<------------------------|  response as HTML fragment     |
  |                         |                                |
  |  SSE: new mail push     |  JMAP EventSource bridge       |
  |<........................|<...............................|
```

JMAP clients are cached per user to avoid re-authentication on each request. Emails are sent using JMAP's two-step pattern: `Email/set` creates the message in Drafts, then `EmailSubmission/set` submits it and moves it to Sent.

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (2024 edition, `#![forbid(unsafe_code)]`) |
| HTTP framework | axum |
| Service framework | [acton-service](https://govcraft.github.io/acton-service/) |
| Protocol | JMAP ([jmap-client](https://crates.io/crates/jmap-client)) |
| Templates | Askama (compiled, type-checked at build time) |
| Interactivity | HTMX + server-sent events |
| Styling | Tailwind CSS v4 |
| Rich text editor | Trix |
| HTML sanitization | Ammonia |
| Asset embedding | rust-embed |

## Development

Missive uses standard Rust tooling with a Tailwind CSS build step for frontend assets.

### Build from Source

```bash
pnpm install
pnpm run build          # Build vendor assets + CSS
cargo check             # Quick compile verification
cargo build             # Full build

# Watch mode for CSS changes (run in a separate terminal)
pnpm run dev:css
```

### Running Tests

```bash
cargo nextest run                        # All tests
cargo nextest run jmap::tests            # Module-specific tests
cargo nextest run parse_recipient_emails # Tests matching a name
```

### Linting

```bash
cargo clippy
```

The codebase enforces `#![forbid(unsafe_code)]` and `#![deny(clippy::unwrap_used, clippy::expect_used)]`. Clippy lints are always fixed at the source, never suppressed with directives.

## Contributing

Contributions are welcome. To get started:

1. Fork the repository and create a feature branch
2. Run `cargo clippy` and `cargo nextest run` before submitting
3. Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages
4. Open a pull request with a clear description of the change

## License

Missive is dual-licensed:

- **[GNU Affero General Public License v3.0 (AGPL-3.0)](https://www.gnu.org/licenses/agpl-3.0.html)** -- free for open-source use, self-hosting, and modification
- **Commercial license** -- available for organizations that need terms beyond AGPL-3.0, including bundling, OEM distribution, or proprietary modifications

For commercial licensing inquiries, contact [Govcraft](https://govcraft.ai).

Copyright (c) 2025-2026 Govcraft
