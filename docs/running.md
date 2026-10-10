# Running aria-er

Commands, outputs and the container stack.

## How to run

Requirements: a stable Rust toolchain (edition 2021), [uv](https://docs.astral.sh/uv/) and `make`.

```bash
make ingest                                   # last 30 days (reprocessing window)
make ingest FROM=2026-08-01 TO=2026-08-31     # explicit window, inclusive
make transform                                # dbt build (seeds, snapshot, models, tests) + source freshness
make orchestrate                              # Dagster UI + daemon on http://localhost:3000
make backfill FROM=2026-08-01 TO=2026-08-31   # ingestion + dbt for a range of days, as one run
make dashboard                                # Streamlit dashboard on http://localhost:8501
make test                                     # Rust tests (no network) + dbt build with its data tests + pytest
make lint                                     # cargo fmt + clippy, ruff + mypy --strict
```

`make ingest` runs `aq-ingest run`, `aq-ingest weather` and `aq-ingest archive`, all taking
`[--from YYYY-MM-DD] [--to YYYY-MM-DD]`. The first downloads the station registry and the
measurements of the window, the second the hourly weather at the station coordinates, the
third loads the validated archive files found in `data/samples/arpae/`:

```
raw/arpae/measurements/year=YYYY/month=MM/part-0.parquet
raw/arpae/measurements_archive/year=YYYY/month=MM/part-0.parquet
raw/arpae/stations/extracted_on=YYYY-MM-DD/stations.parquet
raw/arpae/station_types/extracted_on=YYYY-MM-DD/station_types.parquet
raw/openmeteo/weather/year=YYYY/month=MM/part-0.parquet
```

A full month for the whole region is ~113k rows and takes about 4 minutes, almost all of
it waiting for the ARPAE API (~40 s per request).

Configuration lives in [ingestor/config/default.toml](../ingestor/config/default.toml)
(endpoints, timeouts, retries, output paths, reprocessing window, log format). Any value
can be overridden with an environment variable `AQ_<SECTION>__<KEY>`, for example
`AQ_RUN__REPROCESS_WINDOW_DAYS=45` or `AQ_LOG__FORMAT=json`.

### Raw measurements schema

| Column | Type | Notes |
|---|---|---|
| `station_id` | uint32 | Canonical station code (`7000014`) |
| `pollutant_id` | uint32 | ARPAE parameter id (5 = PM10, 8 = NO2, …) |
| `measured_at` | timestamp (µs, UTC) | Published reference time converted to UTC |
| `value` | float64 | As published, not converted |
| `unit` | string, nullable | Unit of `value` from the pollutant registry |
| `validation_flag` | string | Source flag, verbatim (`M` / `G`) |
| `raw_reftime` | string | Reference time exactly as published |

Natural key: `(station_id, pollutant_id, measured_at)`. Rows are sorted by key.

## Running in containers

```bash
make up      # build the images and start Dagster and the dashboard
make down    # stop them; data stays in the Docker volumes
```

`make up` needs only Docker. It starts two services and waits until both are healthy:

| Service | URL | Image |
|---|---|---|
| `orchestration` | http://localhost:3000 | Dagster with the dbt project and the `aq-ingest` binary |
| `dashboard` | http://localhost:8501 | Streamlit |

On a fresh clone the volumes are empty and the stack fills them on its own: a Dagster
sensor notices that there is no warehouse, loads the archive, then ingests the last 30
days and builds the models. About five minutes after `make up` the dashboard shows ten
years of data; until then it says that the warehouse is not there yet. A one-off ingestion
without Dagster is also available:

```bash
docker compose run --rm ingestor run --from 2026-08-01 --to 2026-08-31
```

- **Images** are multi-stage, in `docker/`. The ingestor image is the Rust binary on a
  distroless base (67 MB); the two Python images install their locked dependencies with uv.
- **Volumes**: `raw` (Parquet written by the ingestor), `warehouse` (the DuckDB file,
  written by dbt and read by the dashboard) and `dagster_home` (run history).
- All containers run as the same unprivileged user, so they can share the volumes.
