<p align="center">
  <img src="branding/missive_logomark.png" alt="Missive" width="128" height="128">
</p>

<p align="center">
  A fast, self-hostable webmail client built with Rust and the JMAP protocol.
</p>

<p align="center">
  <a href="#license"><img src="https://img.shields.io/badge/license-AGPL--3.0-blue" alt="License: AGPL-3.0"></a>
  <img src="https://img.shields.io/badge/rust-2024_edition-orange" alt="Rust 2024 Edition">
  <img src="https://img.shields.io/badge/JMAP-RFC_8620-green" alt="JMAP RFC 8620">
</p>

---

## Overview

Missive is a webmail client that ships as a single binary with all assets embedded. It communicates with your mail server over [JMAP](https://jmap.io/) (RFC 8620), the modern replacement for IMAP, and renders a responsive three-pane UI entirely server-side with HTMX -- no JavaScript framework required.

Missive is built on [acton-service](https://govcraft.github.io/acton-service/), a Rust service framework that provides configuration, session management, health checks, and HTMX integration out of the box. Built and tested against [Stalwart Mail Server](https://stalw.art/), Missive works with any JMAP-compliant mail server.

## Features

- **Three-pane layout** -- mailbox sidebar, message list, and reading pane in a single view
- **Compose, reply, forward** -- rich text editor with file attachments and CC/BCC fields
- **Search** -- full-text search across mailbox contents
- **Keyboard shortcuts** -- navigate, compose, reply, delete, and archive without a mouse
- **Star and flag** -- mark important messages with `$flagged` support
- **Bulk actions** -- multi-select messages for delete, move, or read/unread toggling
- **Archive and spam** -- one-click archive or spam classification
- **Move between folders** -- organize messages across mailboxes
- **Real-time updates** -- server-sent events (SSE) push new mail notifications instantly
- **HTML sanitization** -- safe rendering of HTML emails with Ammonia-based filtering
- **Single binary** -- all static assets embedded via `rust-embed`; no file tree to manage
- **Session backends** -- in-memory sessions for development or Redis for production persistence
- **Dual deployment** -- run locally as a standalone binary or behind a reverse proxy with Docker

## Screenshots

> Screenshots coming soon. Missive features a clean, branded three-pane interface with a dark sidebar, message list, and reading pane.

## Getting Started

### Prerequisites

- A running JMAP-compliant mail server (e.g., [Stalwart](https://stalw.art/))
- For building from source: Rust 1.85+ (2024 edition), Node.js 22+, and [pnpm](https://pnpm.io/)

### Quick Start: Binary

Build and run Missive directly on your machine.

```bash
# Clone the repository
git clone https://github.com/Govcraft/missive.git
cd missive

# Install JS dependencies and build static assets
pnpm install
pnpm run build

# Build the release binary
cargo build --release

# Create a minimal config
cat > config.toml << 'EOF'
jmap_url = "https://your-mail-server.example.com"

[service]
name = "missive"
port = 8080
EOF

# Run
./target/release/missive
```

Open `http://localhost:8080` and log in with your mail server credentials.

### Quick Start: Docker

```bash
# Run with Docker, passing config via environment variables
docker build -t missive .
docker run -p 8080:8080 \
  -e ACTON_JMAP_URL=https://your-mail-server.example.com \
  missive
```

The Docker image uses a multi-stage build that produces a minimal Debian-based runtime image. A built-in health check pings `/health` every 30 seconds.

## Configuration

Missive is configured through a `config.toml` file, environment variables, or both. Configuration is powered by acton-service's [Figment-based config system](https://govcraft.github.io/acton-service/docs/configuration). Environment variables take precedence and use the `ACTON_` prefix (the convention from acton-service).

### Config File

```toml
jmap_url = "https://mail.example.com"
page_size = 50

[service]
name = "missive"
port = 8080

[session]
storage = "memory"              # "memory" or "redis"
redis_url = "redis://localhost:6379"  # required when storage = "redis"
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

**Redis** sessions persist across restarts and support multi-instance deployments. Enable by setting `session.storage = "redis"` and providing a `redis_url`.

## Deployment

### Docker Compose with Traefik

The included `docker-compose.yml` sets up Missive behind Traefik with automatic HTTPS via Let's Encrypt.

```bash
# Create a .env file
cat > .env << 'EOF'
DOMAIN=mail.example.com                              # Public domain Traefik will route to Missive
ACME_EMAIL=admin@example.com                         # Email for Let's Encrypt certificate notifications
ACTON_JMAP_URL=https://your-mail-server.example.com  # Your JMAP mail server URL
EOF

# Start the stack
docker compose up -d
```

To enable Redis-backed sessions, add these to your `.env` file and activate the Redis profile:

```bash
# Append to .env
echo 'ACTON_SESSION_STORAGE=redis' >> .env
echo 'ACTON_SESSION_REDIS_URL=redis://redis:6379' >> .env

# Start with the Redis profile
docker compose --profile redis up -d
```

### Systemd Service

A unit file is provided at `deploy/missive.service` for running Missive as a system service.

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

The unit runs with `ProtectSystem=strict`, `ProtectHome=true`, and `NoNewPrivileges=true` for hardened security.

### Reverse Proxy

Missive binds to `0.0.0.0:8080` by default. When running behind a reverse proxy (nginx, Caddy, Traefik), forward traffic to that port. Missive serves the `/health` endpoint for liveness probes and `/ready` for readiness checks.

Key proxy considerations:
- **SSE support** -- disable response buffering for `/api/v1/events` so server-sent events stream correctly
- **Timeouts** -- SSE connections are long-lived; set proxy read timeouts accordingly
- **WebSocket** -- not required; Missive uses SSE, not WebSockets

## Development

### Build from Source

```bash
# Install dependencies
pnpm install

# Build CSS (required before cargo build, since assets are embedded)
pnpm run build

# Quick compile check
cargo check

# Full build
cargo build

# Run with live CSS rebuilding (in a separate terminal)
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

### Project Structure

```
missive/
  src/
    main.rs          # Route registration, app startup, session layer
    config.rs        # MissiveConfig with jmap_url and page_size
    session.rs       # Session types, AuthenticatedClient extractor
    jmap.rs          # All JMAP protocol operations and type-safe ID newtypes
    sanitize.rs      # Ammonia-based HTML sanitization for email rendering
    error.rs         # MissiveError enum with HTMX-aware responses
    assets.rs        # rust-embed static asset serving
    routes/
      auth.rs        # Login and logout handlers
      pages.rs       # Full page renders (inbox, calendar, contacts)
      emails.rs      # Email CRUD, compose, reply, forward, attachments
      mailboxes.rs   # Mailbox sidebar listing
      events.rs      # SSE endpoint bridging JMAP EventSource
  templates/         # Askama HTML templates (base layout + HTMX partials)
  static/
    css/input.css    # Tailwind v4 source (the only CSS file to edit)
  deploy/            # Systemd unit file
  branding/          # Logo and logomark assets
```

### Architecture

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

Missive acts as a thin translation layer between HTMX in the browser and JMAP on the server. Route handlers receive an `AuthenticatedClient` extractor that gates every API call behind a valid session and caches JMAP clients per user to avoid re-authentication on each request.

Emails are sent using JMAP's two-step pattern: `Email/set` creates the message in Drafts, then `EmailSubmission/set` submits it with `onSuccessUpdateEmail` to move it to Sent.

**Tech stack:**

| Layer | Technology |
|-------|-----------|
| Language | Rust (2024 edition, no `unsafe`) |
| HTTP framework | axum |
| Service framework | [acton-service](https://govcraft.github.io/acton-service/) |
| Protocol | JMAP (jmap-client crate) |
| Templates | Askama (compiled, type-checked) |
| Interactivity | HTMX + SSE |
| Styling | Tailwind CSS v4 |
| Rich text editor | Trix |
| HTML sanitization | Ammonia |
| Asset embedding | rust-embed |

## Contributing

Contributions are welcome. To get started:

1. Fork the repository and create a feature branch
2. Run `cargo clippy` and `cargo nextest run` before submitting
3. Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages
4. Open a pull request with a clear description of the change

Please fix clippy lints rather than suppressing them. The project values a clean, auditable codebase.

## License

Missive is dual-licensed:

- **[GNU Affero General Public License v3.0 (AGPL-3.0)](https://www.gnu.org/licenses/agpl-3.0.html)** -- free for open-source use
- **Commercial license** -- available for organizations that need an alternative to AGPL-3.0 terms (details to follow)

Copyright (c) 2025-2026 Govcraft
