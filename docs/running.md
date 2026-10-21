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
make fill-gaps MONTHS=6                       # months between the archive and the daily window that raw/ lacks
make dashboard                                # Streamlit dashboard on http://localhost:8501
make test                                     # Rust tests (no network) + dbt build with its data tests + pytest
make lint                                     # cargo fmt + clippy, ruff + mypy --strict
```

`make ingest` runs `aq-ingest run`, `aq-ingest weather` and `aq-ingest archive`, all taking
`[--from YYYY-MM-DD] [--to YYYY-MM-DD]`. The first downloads the station registry, the station types
and the measurements of the window from the ARPAE REST archive, the second the hourly weather at the station coordinates, the
third loads the validated archive files found in `data/samples/arpae/`:

```
raw/arpae/measurements/year=YYYY/month=MM/part-0.parquet
raw/arpae/measurements_archive/year=YYYY/month=MM/part-0.parquet
raw/arpae/stations/extracted_on=YYYY-MM-DD/stations.parquet
raw/arpae/station_types/extracted_on=YYYY-MM-DD/station_types.parquet
raw/openmeteo/weather/year=YYYY/month=MM/part-0.parquet
```

A full month for the whole region is about 175k rows in 90 pages and takes 4 to 6 minutes. A page usually takes a
few seconds, at times a minute, and connections drop now and then (the retries cover it).

Measurements ingested before 2026-10-11 came from the CKAN datastore and have PM10 and
PM2.5 one day late ([data.md](data.md)). To replace them, remove `raw/arpae/measurements/`,
ingest the period again and rebuild the incremental model:

```bash
make ingest FROM=2026-01-01
cd transform && uv run dbt build --profiles-dir . --full-refresh
```

Configuration lives in [ingestor/config/default.toml](../ingestor/config/default.toml)
(endpoints, timeouts, retries, output paths, reprocessing window, log format). Any value
can be overridden with an environment variable `AQ_<SECTION>__<KEY>`, for example
`AQ_RUN__REPROCESS_WINDOW_DAYS=45` or `AQ_LOG__FORMAT=json`.

### Raw measurements schema

| Column | Type | Notes |
|---|---|---|
| `station_id` | uint32 | Canonical station code (`7000014`) |
| `pollutant_id` | uint32 | ARPAE parameter id (5 = PM10, 8 = NO2, …) |
| `measured_at` | timestamp (µs, UTC) | End of the hour for hourly values, start of the day for daily ones |
| `value` | float64 | As published, not converted |
| `unit` | string, nullable | Unit of `value` as published (`µg/m³`, `mg/m³`; `ug/m3`, `mg/m3` in the yearly files) |
| `validation_flag` | string, nullable | Validation stages as published (`Giornaliera + Mensile`); empty for the yearly files |
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
