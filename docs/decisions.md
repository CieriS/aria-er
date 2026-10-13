# Design decisions

Decisions by phase, with the alternatives discarded. Architecture Decision Records are in [adr/](adr/).

## Architecture Decision Records

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-parquet-upsert-by-partition-rewrite.md) | Upsert on Parquet by rewriting whole partitions | accepted |
| [0002](adr/0002-near-real-time-datastore-as-phase-1-source.md) | Near-real-time datastore as the measurement source, UTC+1 timestamps | superseded by 0008 |
| [0003](adr/0003-historical-archive-read-by-dbt.md) | Archive read by dbt from CSV | superseded by 0005 |
| [0004](adr/0004-generic-source-and-sink-traits.md) | `Source` and `Sink` generic over the record type | accepted |
| [0005](adr/0005-archive-loaded-by-the-ingestor.md) | Archive loaded by the ingestor into its own raw dataset | accepted |
| [0006](adr/0006-gcs-and-bigquery-as-a-second-target.md) | Cloud Storage and BigQuery as a second target of the same pipeline | accepted, not yet run on GCP |
| [0007](adr/0007-no-streaming.md) | Streaming is not justified for these data | accepted |
| [0008](adr/0008-rest-archive-as-the-measurement-source.md) | Measurements from the REST archive, ids and reference times translated by the adapter | accepted |

## Phase 1 — Rust ingestor

- **Why Rust, for a job that waits on the network.** Not for speed: a month of measurements
  is about 175,000 rows and nearly all of a run is spent waiting for ARPAE, so a Python
  script would finish in the same time. There is no benchmark in this repository and none is
  claimed. The reasons are two.
  The first is the shape the project wants to show: orchestration and modelling in Python and
  SQL, the part that touches bytes in a compiled language. It is the split the data tooling
  itself has taken (Arrow, Polars, DataFusion, and `uv` and `ruff` used here), and the
  ingestor writes Parquet through the same `arrow` and `parquet` crates those tools are built
  on.
  The second is that it was a deliberate way to learn the language on a real problem instead
  of an exercise, with the compiler as a strict reviewer.
  What it gives here, and can be pointed at: sources and sinks are traits, so pagination,
  retries and parsing are tested without a network and the measurement source was replaced
  without touching the sink or the CLI ([ADR 0008](adr/0008-rest-archive-as-the-measurement-source.md));
  a record that does not match the expected format stops the run with an error naming the
  record, because a missing or mistyped field is not representable; Dagster calls one
  executable with a window and reads its counters, with no Python dependencies shared with
  dbt or Dagster.
  What it costs: a second toolchain in the images and in the CI, slower builds, and more
  code than the Python equivalent. For an ingestor alone, with these volumes and no interest
  in the language, Python would be the reasonable choice.
- **Upsert by rewriting partitions** ([ADR 0001](adr/0001-parquet-upsert-by-partition-rewrite.md)).
  Each run merges the incoming rows into the monthly files it touches, by natural key,
  and rewrites them sorted. Re-running a window yields byte-identical files and never
  duplicates; a revised value replaces the provisional one.
- **Near-real-time datastore as the only source for now**
  ([ADR 0002](adr/0002-near-real-time-datastore-as-phase-1-source.md)). One SQL query
  per window with keyset pagination; timestamps treated as fixed UTC+1 and converted to
  UTC; the original string is kept so the conversion can be redone. Replaced in October
  2026 by the REST archive, behind the same trait
  ([ADR 0008](adr/0008-rest-archive-as-the-measurement-source.md)): the datastore had
  stopped, and the new source needed its ids and reference times translated.
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

- **One run per range, not one per day** (`BackfillPolicy.single_run`). Every run downloads
  the registries again and the measurements come in pages of almost a day each: a month as
  31 runs would repeat the fixed cost 31 times for the same result.
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
- **Station type ingested, not typed in**: the registry spreadsheet does not say whether a
  station measures traffic or background. It was first read from the daily bulletin; when
  ARPAE dropped the exposure from the bulletin label (2026-10-06),
  `mart_traffic_vs_background` emptied without failing any test. The type is now read from
  the station registry of the ARPAE REST service (`qa_stazioni`), with the same label form
  so that old and new snapshots read alike; an error-level test requires both traffic and
  background stations to exist, and a frozen reference seed covers the case of a source
  that stops stating the exposure.
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

## Phase 7 — cloud (GCP)

- **Storage backend inside the sink, GCS behind a Cargo feature**, external tables in
  BigQuery, and one set of models with the dialect differences in macros
  ([ADR 0006](adr/0006-gcs-and-bigquery-as-a-second-target.md)).
- **No compute in the cloud.** The ingestor and dbt are run with credentials from wherever
  they already run; the cloud holds only data.
- **Service accounts without keys in Terraform.** A key in the state would be a secret in
  a bucket; keys are created by hand when needed.
- **`us-central1`**, because the Cloud Storage free tier does not cover European regions.
- **Two Terraform passes**: external tables are enabled after the first ingestion, since
  BigQuery needs files to exist under their prefix.
- **CI plans, never applies**, and only where the repository has the GCP settings.

## Phase 8 — streaming (not built)

- **No streaming** ([ADR 0007](adr/0007-no-streaming.md)). The sources are batches polled
  over HTTP, published hours after measurement and revised for months; every question the
  platform answers is daily or coarser. If fresher data is ever needed, the idempotent
  schedule can simply run more often.
