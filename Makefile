MANIFEST := --manifest-path ingestor/Cargo.toml
CONFIG ?= ingestor/config/default.toml

# Optional window: make ingest FROM=2026-08-01 TO=2026-08-31
WINDOW := $(if $(FROM),--from $(FROM)) $(if $(TO),--to $(TO))

DBT := cd transform && uv run dbt
DAGSTER_HOME := $(CURDIR)/orchestration/.dagster_home
DAGSTER := cd orchestration && DAGSTER_HOME=$(DAGSTER_HOME) uv run dagster

.PHONY: build ingest transform orchestrate backfill dashboard test lint

build:
	cargo build $(MANIFEST) --release --bin aq-ingest

# ARPAE measurements and registry, then Open-Meteo weather, for the same window;
# then the whole local archive (a no-op when nothing changed).
ingest: build
	ingestor/target/release/aq-ingest --config $(CONFIG) run $(WINDOW)
	ingestor/target/release/aq-ingest --config $(CONFIG) weather $(WINDOW)
	ingestor/target/release/aq-ingest --config $(CONFIG) archive

# Builds seeds, snapshots, models and runs their tests, then reports source freshness.
transform:
	mkdir -p warehouse
	$(DBT) build --profiles-dir .
	$(DBT) source freshness --profiles-dir .

# dbt build on the committed fixtures, in a throwaway database: no network, no raw/ needed.
# Runs twice so that the incremental path is exercised too. This is what the CI runs.
.PHONY: transform-fixtures
FIXTURE_DB := $(or $(RUNNER_TEMP),/tmp)/aria_er_fixtures.duckdb
FIXTURE_VARS := {raw_dir: fixtures/raw}
transform-fixtures:
	rm -f $(FIXTURE_DB)
	cd transform && AQ_DUCKDB_PATH=$(FIXTURE_DB) uv run dbt build --profiles-dir . --vars '$(FIXTURE_VARS)'
	cd transform && AQ_DUCKDB_PATH=$(FIXTURE_DB) uv run dbt build --profiles-dir . --vars '$(FIXTURE_VARS)'

# Dagster UI on http://localhost:3000, with the daemon that runs schedules and backfills.
orchestrate: build
	mkdir -p $(DAGSTER_HOME)
	$(DAGSTER) dev -m aria_er_orchestration.definitions

# The local archive first, then ingestion + dbt for a range of days as one run:
# make backfill FROM=2026-08-01 TO=2026-08-31
backfill: build
	@test -n "$(FROM)" -a -n "$(TO)" || (echo "usage: make backfill FROM=YYYY-MM-DD TO=YYYY-MM-DD" && exit 1)
	mkdir -p $(DAGSTER_HOME)
	$(DAGSTER) job execute -m aria_er_orchestration.definitions -j arpae_archive_load
	$(DAGSTER) asset materialize -m aria_er_orchestration.definitions --select 'not key:"arpae_raw/measurements_archive"' --partition-range $(FROM)...$(TO)

# Streamlit dashboard on http://localhost:8501, reading the marts built by `make transform`.
dashboard:
	cd dashboard && uv run streamlit run src/aria_er_dashboard/app.py --server.port 8501

# The dbt tests run against the data in raw/: run `make ingest` at least once before.
test:
	cargo test $(MANIFEST)
	mkdir -p warehouse
	$(DBT) build --profiles-dir .
	cd orchestration && uv run pytest -q
	cd dashboard && uv run pytest -q

lint:
	cargo fmt $(MANIFEST) --all -- --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
	cd orchestration && uv run ruff format --check . && uv run ruff check . && uv run mypy
	cd dashboard && uv run ruff format --check . && uv run ruff check . && uv run mypy

# The whole system in containers: Dagster on http://localhost:3000, dashboard on
# http://localhost:8501. Data lives in Docker volumes and survives `make down`.
.PHONY: up down
up:
	docker compose up --build --detach --wait
	@echo "Dagster: http://localhost:3000  -  dashboard: http://localhost:8501"

down:
	docker compose down

# --- Cloud (GCP): see docs/cloud.md. Needs credentials in the environment and
# AQ_GCP_PROJECT, AQ_GCS_BUCKET set to the project and to the raw bucket. ---
.PHONY: cloud-ingest cloud-transform cloud-compare

# Same ingestion as `make ingest`, written to the bucket instead of raw/.
cloud-ingest:
	@test -n "$(AQ_GCS_BUCKET)" || (echo "set AQ_GCS_BUCKET to the raw bucket" && exit 1)
	cargo build $(MANIFEST) --release --bin aq-ingest --features gcs
	AQ_SINK__STORAGE=gcs AQ_SINK__GCS_BUCKET=$(AQ_GCS_BUCKET) ingestor/target/release/aq-ingest --config $(CONFIG) run $(WINDOW)
	AQ_SINK__STORAGE=gcs AQ_SINK__GCS_BUCKET=$(AQ_GCS_BUCKET) ingestor/target/release/aq-ingest --config $(CONFIG) weather $(WINDOW)
	AQ_SINK__STORAGE=gcs AQ_SINK__GCS_BUCKET=$(AQ_GCS_BUCKET) ingestor/target/release/aq-ingest --config $(CONFIG) archive

# The same dbt models, built in BigQuery.
cloud-transform:
	@test -n "$(AQ_GCP_PROJECT)" || (echo "set AQ_GCP_PROJECT to the GCP project" && exit 1)
	cd transform && uv run --group cloud dbt build --profiles-dir . --target bigquery

# Checks that mart_exceedances_yearly holds the same rows in DuckDB and in BigQuery.
cloud-compare:
	cd transform && uv run --group cloud python scripts/compare_targets.py
