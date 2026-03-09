# Stage 1: Build static assets and Rust binary
FROM rust:1-bookworm AS builder

# Install Node.js and pnpm for CSS/vendor build
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && corepack enable && corepack prepare pnpm@latest --activate \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install JS dependencies first (layer cache)
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

# Build CSS and vendor assets
COPY static/css/input.css static/css/input.css
COPY static/images/ static/images/
COPY static/favicon.ico static/favicon.ico
COPY templates/ templates/
RUN mkdir -p static/js static/css \
    && pnpm run build

# Cache Rust dependencies (layer cache)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/missive target/release/deps/missive-*

# Build the actual binary (embeds static/ via rust-embed)
COPY src/ src/
RUN cargo build --release

# Stage 2: Minimal runtime
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home missive

COPY --from=builder /app/target/release/missive /usr/local/bin/missive

USER missive
WORKDIR /home/missive

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

CMD ["missive"]
