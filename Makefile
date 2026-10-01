MANIFEST := --manifest-path ingestor/Cargo.toml
CONFIG ?= ingestor/config/default.toml

# Optional window: make ingest FROM=2026-08-01 TO=2026-08-31
WINDOW := $(if $(FROM),--from $(FROM)) $(if $(TO),--to $(TO))

.PHONY: ingest test lint

ingest:
	cargo run $(MANIFEST) --release --bin aq-ingest -- --config $(CONFIG) run $(WINDOW)

test:
	cargo test $(MANIFEST)

lint:
	cargo fmt $(MANIFEST) --all -- --check
	cargo clippy $(MANIFEST) --all-targets -- -D warnings
