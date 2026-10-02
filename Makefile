MANIFEST := --manifest-path ingestor/Cargo.toml
CONFIG ?= ingestor/config/default.toml

# Optional window: make ingest FROM=2026-08-01 TO=2026-08-31
WINDOW := $(if $(FROM),--from $(FROM)) $(if $(TO),--to $(TO))

DBT := cd transform && uv run dbt

.PHONY: ingest transform test lint

ingest:
	cargo run $(MANIFEST) --release --bin aq-ingest -- --config $(CONFIG) run $(WINDOW)

# Builds seeds, snapshots, models and runs their tests, then reports source freshness.
transform:
	mkdir -p warehouse
	$(DBT) build --profiles-dir .
	$(DBT) source freshness --profiles-dir .

# The dbt tests run against the data in raw/: run `make ingest` at least once before.
test:
	cargo test $(MANIFEST)
	mkdir -p warehouse
	$(DBT) build --profiles-dir .

lint:
	cargo fmt $(MANIFEST) --all -- --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
