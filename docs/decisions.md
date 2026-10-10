# Design decisions

Decisions by phase, with the alternatives discarded. Architecture Decision Records are in [adr/](adr/).

## Architecture Decision Records

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-parquet-upsert-by-partition-rewrite.md) | Upsert on Parquet by rewriting whole partitions | accepted |
| [0002](adr/0002-near-real-time-datastore-as-phase-1-source.md) | Near-real-time datastore as the measurement source, UTC+1 timestamps | accepted |
| [0003](adr/0003-historical-archive-read-by-dbt.md) | Archive read by dbt from CSV | superseded by 0005 |
| [0004](adr/0004-generic-source-and-sink-traits.md) | `Source` and `Sink` generic over the record type | accepted |
| [0005](adr/0005-archive-loaded-by-the-ingestor.md) | Archive loaded by the ingestor into its own raw dataset | accepted |

## Phase 1 — Rust ingestor

- **Upsert by rewriting partitions** ([ADR 0001](adr/0001-parquet-upsert-by-partition-rewrite.md)).
  Each run merges the incoming rows into the monthly files it touches, by natural key,
  and rewrites them sorted. Re-running a window yields byte-identical files and never
  duplicates; a revised value replaces the provisional one.
- **Near-real-time datastore as the only source for now**
  ([ADR 0002](adr/0002-near-real-time-datastore-as-phase-1-source.md)). One SQL query
  per window with keyset pagination; timestamps treated as fixed UTC+1 and converted to
  UTC; the original string is kept so the conversion can be redone.
- **Sources and sinks behind traits** (`Source`, `Sink` in `aq-core`). The HTTP layer is
  a further small trait, so retry, pagination and parsing are tested without network.
- **Blocking HTTP (`ureq`) instead of an async stack.** The ingestor makes a handful of
  sequential requests: an async runtime would add dependencies without any benefit.
- **`arrow` + `parquet` crates instead of Polars or a table format.** Only typed columnar
  write/read is needed; Delta/Iceberg would be oversized at this volume.
- **Fail loudly on unexpected formats.** A row with an unparsable date, value or station
  code aborts the run instead of being skipped.

## Phase 2 — dbt on DuckDB

- **The validated archive wins over the near-real-time feed** for the same key. Yearly
  counts need full years, which only the archive has. It was first read by dbt straight
  from CSV ([ADR 0003](adr/0003-historical-archive-read-by-dbt.md)); it is now loaded by
  the ingestor into its own raw dataset
  ([ADR 0005](adr/0005-archive-loaded-by-the-ingestor.md)).
- **One incremental fact, `delete+insert` on the natural key.** Aggregates on top are
  plain tables: at this volume a full rebuild takes under a second, and incremental
  aggregates would have to handle late revisions.
- **No dbt packages.** The only helper needed, a multi-column uniqueness test, is a
  five-line generic test in the project.
- **Limits and pollutant metadata as seeds.** No threshold, pollutant id or averaging
  period is written in a model; the ozone model selects its pollutant from the seed.
- **Completeness against an expected grid.** Missing hours are absent rows, so coverage
  is computed against every day between the first and last observation of a series.

## Phase 3 — Dagster orchestration

- **One run per range, not one per day** (`BackfillPolicy.single_run`). An ARPAE query takes
  ~40 s whatever the window: a month as 31 runs would take over half an hour of waiting
  against a few minutes, for the same result.
- **dbt assets share the daily partitions.** The models are not physically partitioned;
  the partition range only tells dbt how far back to reprocess (`reprocess_from` variable),
  so a backfill older than the 30-day lookback still reaches the incremental model.
- **The binary is called through a small resource**, not Dagster Pipes or a Python port:
  the ingestor is a plain CLI, its JSON log is enough to report counters and errors.
- **Raw assets are one multi-asset** because a single `aq-ingest` call writes both the
  measurements and the registry snapshot.
- **Checks read the data, not Dagster metadata**: freshness looks at the newest
  `measured_at` in the Parquet files, which is what matters when the source itself is stale.

## Phase 4 — Open-Meteo weather

- **`Source` and `Sink` generic over the record type**
  ([ADR 0004](adr/0004-generic-source-and-sink-traits.md)). The traits were shaped
  around ARPAE; weather is a different record. One trait with an associated type, one
  generic upserting sink and one pipeline function now serve both sources.
- **Coordinates deduplicated by rounding**, not by clustering: deterministic, trivially
  reproducible in SQL, and matched to the resolution of the weather model.
- **Wide weather rows** (one row per hour with all variables), as the API returns them,
  instead of one row per variable.
- **Coordinates come from the live registry**, so the weather command has no dependency on
  files written by a previous run.
- **Fixed thresholds as dbt variables** for windy (3 m/s) and rainy (1 mm) days, and no
  correlation reported under 20 days.

## Phase 5 — Streamlit dashboard

- **Three thin layers** (data access, chart builders, pages). The first two are pure
  functions tested without Streamlit; pages are exercised with Streamlit's `AppTest` on a
  small synthetic warehouse.
- **Two marts added for the dashboard** (`mart_pollutant_trend`,
  `mart_traffic_vs_background`) rather than aggregating in Python.
- **Station type ingested, not typed in**: the ARPAE registry does not say whether a
  station measures traffic or background; the daily bulletin does. The ingestor snapshots
  it at every run, so a reclassification by ARPAE reaches the marts on its own.
- **Altair** for charts: it ships with Streamlit, no extra plotting dependency.
- **A connection per query, read-only**: no state shared between sessions, and the
  dashboard cannot modify the warehouse.

## Phase 6 — containers and CI

- **One Dagster container** running `dagster dev` (webserver and daemon together) instead of
  separate services with a Postgres run storage: enough for a single-machine stack.
- **The orchestration image carries the binary and the dbt project**, because Dagster calls
  both. The ingestor image exists on its own for one-off runs and as the minimal artifact.
- **Named volumes, not bind mounts**, so a clone needs no local directories or permissions.
- **dbt in CI runs on committed fixtures** (a small real slice written with the ingestor's
  schema), built twice to cover the incremental path, with no network.
