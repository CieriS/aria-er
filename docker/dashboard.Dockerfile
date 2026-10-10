# syntax=docker/dockerfile:1

# --- dependencies: a virtual environment with Streamlit ---
FROM python:3.14-slim-bookworm AS dependencies
COPY --from=ghcr.io/astral-sh/uv:0.12 /uv /usr/local/bin/uv
ENV UV_LINK_MODE=copy
WORKDIR /app/dashboard
COPY dashboard/pyproject.toml dashboard/uv.lock ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-dev --no-install-project
COPY dashboard/src ./src
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --locked --no-dev

# --- runtime: the dashboard, reading the warehouse volume ---
FROM python:3.14-slim-bookworm AS runtime
RUN useradd --create-home --uid 1000 aria
WORKDIR /app/dashboard
COPY --from=dependencies --chown=aria:aria /app/dashboard /app/dashboard
ENV PATH=/app/dashboard/.venv/bin:$PATH \
    AQ_DUCKDB_PATH=/app/warehouse/aria_er.duckdb \
    PYTHONUNBUFFERED=1
USER aria
EXPOSE 8501
CMD ["streamlit", "run", "src/aria_er_dashboard/app.py", "--server.port", "8501", "--server.address", "0.0.0.0", "--server.headless", "true"]
