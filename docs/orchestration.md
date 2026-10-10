# Orchestration (Dagster)

Assets, partitions, schedule, sensor, retries and checks.

## Orchestration (Dagster)

`make orchestrate` starts the Dagster UI and its daemon. The asset graph is the whole
pipeline: the two raw assets written by `aq-ingest`, then every dbt seed, snapshot and model
downstream of them.

- **Ingestion assets** (`arpae_raw/measurements`, `arpae_raw/stations`,
  `arpae_raw/station_types`, `openmeteo_raw/weather`): run the `aq-ingest` binary (`run` and
  `weather` subcommands) for the selected days. `arpae_raw/measurements_archive` loads the
  local archive files with `aq-ingest archive`; it is not partitioned and has its own job,
  `arpae_archive_load`. No ingestion logic is duplicated in Python. The counters
  of the run (fetched, inserted, updated rows) are attached to the materialization.
- **dbt assets**: loaded from the dbt manifest with `dagster-dbt`; dbt sources and the raw
  assets share the same keys, which is what connects the lineage. dbt tests appear as asset checks.
- **Daily partitions** on every asset, in ARPAE local standard time. A range of days runs as
  a *single* run (`aq-ingest --from … --to …`, then one `dbt build`), from the UI
  (Materialize → select a range) or with `make backfill`.
- **Schedule** `reprocess_provisional_window`: every day at 06:00 Europe/Rome it re-ingests
  the last 30 days, the window in which ARPAE may still revise data, and rebuilds the models.
  It is on by default; it only fires while the daemon (`make orchestrate`) is running.
- **Sensor** `bootstrap_empty_warehouse`: on an installation without a warehouse it launches
  the archive load and then the refresh of the last 30 days, one step per minute, and does
  nothing afterwards. Each step is launched at most once a day, so a failing step is not
  retried in a loop.
- **Retries**: the ingestion step is retried twice with exponential backoff, on top of the
  HTTP retries inside the ingestor. On failure the run shows the exit code and the last lines
  of the ingestor log.
- **Asset checks**: `raw_measurements_freshness` (newest measurement at most 48 hours old) and
  `recent_data_completeness` (at least 90% of station-pollutant-days complete over the last
  30 days of data). Both have `WARN` severity: they flag the problem without blocking the run.

Verified on real data: a backfill of August 2026 (31 partitions) launched through the
Dagster webserver completed as one run in about 9 minutes — ingestion (which needed one
retry after an ARPAE API failure), `dbt build` and both checks. The freshness check reported
the expected warning, the source being stale at the time.
