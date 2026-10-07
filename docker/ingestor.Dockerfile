# syntax=docker/dockerfile:1

# --- build: compile the aq-ingest binary ---
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY ingestor/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin aq-ingest \
    && cp target/release/aq-ingest /usr/local/bin/aq-ingest \
    && mkdir /raw

# --- runtime: the binary and its configuration, nothing else ---
# TLS roots are compiled into the binary, so no system certificates are needed.
FROM gcr.io/distroless/cc-debian12 AS runtime
WORKDIR /app
# Same numeric user as the other images, so that all of them can write the raw volume.
COPY --from=build --chown=1000:1000 /raw /app/raw
USER 1000:1000
COPY --from=build /usr/local/bin/aq-ingest /usr/local/bin/aq-ingest
COPY ingestor/config/default.toml /app/ingestor/config/default.toml
ENV AQ_CONFIG=/app/ingestor/config/default.toml \
    AQ_LOG__FORMAT=json
# Output paths in the configuration are relative to /app (raw/ is a volume).
ENTRYPOINT ["/usr/local/bin/aq-ingest"]
CMD ["run"]
