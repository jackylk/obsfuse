# Build stage
FROM rust:1.75-bookworm as builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    libfuse3-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create dummy source to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn dummy() {}" > src/lib.rs

# Build dependencies
RUN cargo build --release && rm -rf src target/release/deps/obsfuse*

# Copy actual source
COPY src ./src
COPY benches ./benches
COPY examples ./examples
COPY tests ./tests

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libfuse3-3 \
    fuse3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /app/target/release/obsfuse /usr/local/bin/

# Create mount point
RUN mkdir -p /mnt/obs

# Set default environment variables
ENV OBS_ENDPOINT=obs.cn-north-1.myhuaweicloud.com
ENV OBS_REGION=cn-north-1
ENV LOG_LEVEL=info

# Entry point script
COPY <<'EOF' /entrypoint.sh
#!/bin/bash
set -e

# Check required environment variables
if [ -z "$OBS_BUCKET" ]; then
    echo "Error: OBS_BUCKET environment variable is required"
    exit 1
fi

if [ -z "$OBS_ACCESS_KEY" ]; then
    echo "Error: OBS_ACCESS_KEY environment variable is required"
    exit 1
fi

if [ -z "$OBS_SECRET_KEY" ]; then
    echo "Error: OBS_SECRET_KEY environment variable is required"
    exit 1
fi

# Mount options
MOUNT_OPTS="--endpoint $OBS_ENDPOINT --region $OBS_REGION --log-level $LOG_LEVEL"

if [ -n "$OBS_PREFIX" ]; then
    MOUNT_OPTS="$MOUNT_OPTS --prefix $OBS_PREFIX"
fi

if [ "$ALLOW_OTHER" = "true" ]; then
    MOUNT_OPTS="$MOUNT_OPTS --allow-other"
fi

if [ "$READ_ONLY" = "true" ]; then
    MOUNT_OPTS="$MOUNT_OPTS --read-only"
fi

# Run obsfuse
exec obsfuse mount "$OBS_BUCKET" /mnt/obs $MOUNT_OPTS --foreground
EOF

RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
