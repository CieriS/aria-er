# syntax=docker/dockerfile:1

# --- build: the same aq-ingest build as the ingestor image (shared layer cache) ---
FROM rust:1-slim-bookworm AS ingestor
WORKDIR /src
COPY ingestor/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin aq-ingest \
    && cp target/release/aq-ingest /usr/local/bin/aq-ingest

# --- dependencies: a virtual environment with Dagster and dbt ---
FROM python:3.12-slim-bookworm AS dependencies
COPY --from=ghcr.io/astral-sh/uv:0.12 /uv /usr/local/bin/uv
ENV UV_LINK_MODE=copy
WORKDIR /app/orchestration
COPY orchestration/pyproject.toml orchestration/uv.lock ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-dev --no-install-project
COPY orchestration/src ./src
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-dev

# --- runtime: Dagster with the dbt project and the ingestor binary ---
FROM python:3.12-slim-bookworm AS runtime
RUN useradd --create-home --uid 1000 aria \
    && mkdir -p /app/raw /app/warehouse /app/dagster_home \
    && chown -R aria:aria /app
WORKDIR /app
COPY --from=ingestor /usr/local/bin/aq-ingest /usr/local/bin/aq-ingest
COPY --from=dependencies --chown=aria:aria /app/orchestration /app/orchestration
COPY --chown=aria:aria ingestor/config /app/ingestor/config
COPY --chown=aria:aria transform /app/transform
COPY --chown=aria:aria data/samples /app/data/samples
ENV PATH=/app/orchestration/.venv/bin:$PATH \
    AQ_REPO_ROOT=/app \
    AQ_INGEST_BINARY=/usr/local/bin/aq-ingest \
    DAGSTER_HOME=/app/dagster_home \
    PYTHONUNBUFFERED=1
USER aria
# Parse the dbt project at build time so the container starts with a manifest.
RUN cd /app/transform && dbt parse --profiles-dir . --quiet
WORKDIR /app/orchestration
EXPOSE 3000
CMD ["dagster", "dev", "-h", "0.0.0.0", "-p", "3000", "-m", "aria_er_orchestration.definitions"]
