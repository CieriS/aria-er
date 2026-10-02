MANIFEST := --manifest-path ingestor/Cargo.toml
CONFIG ?= ingestor/config/default.toml

# Optional window: make ingest FROM=2026-08-01 TO=2026-08-31
WINDOW := $(if $(FROM),--from $(FROM)) $(if $(TO),--to $(TO))

DBT := cd transform && uv run dbt
DAGSTER_HOME := $(CURDIR)/orchestration/.dagster_home
DAGSTER := cd orchestration && DAGSTER_HOME=$(DAGSTER_HOME) uv run dagster

.PHONY: build ingest transform orchestrate backfill test lint

build:
	cargo build $(MANIFEST) --release --bin aq-ingest

ingest:
	cargo run $(MANIFEST) --release --bin aq-ingest -- --config $(CONFIG) run $(WINDOW)

# Builds seeds, snapshots, models and runs their tests, then reports source freshness.
transform:
	mkdir -p warehouse
	$(DBT) build --profiles-dir .
	$(DBT) source freshness --profiles-dir .

# Dagster UI on http://localhost:3000, with the daemon that runs schedules and backfills.
orchestrate: build
	mkdir -p $(DAGSTER_HOME)
	$(DAGSTER) dev -m aria_er_orchestration.definitions

# Ingestion + dbt for a range of days, as one run: make backfill FROM=2026-08-01 TO=2026-08-31
backfill: build
	@test -n "$(FROM)" -a -n "$(TO)" || (echo "usage: make backfill FROM=YYYY-MM-DD TO=YYYY-MM-DD" && exit 1)
	mkdir -p $(DAGSTER_HOME)
	$(DAGSTER) asset materialize -m aria_er_orchestration.definitions --select '*' --partition-range $(FROM)...$(TO)

# The dbt tests run against the data in raw/: run `make ingest` at least once before.
test:
	cargo test $(MANIFEST)
	mkdir -p warehouse
	$(DBT) build --profiles-dir .
	cd orchestration && uv run pytest -q

lint:
	cargo fmt $(MANIFEST) --all -- --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
	cd orchestration && uv run ruff format --check . && uv run ruff check . && uv run mypy
