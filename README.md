# aria-er

[![Rust](https://github.com/CieriS/aria-er/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/CieriS/aria-er/actions/workflows/rust.yml)
[![Python](https://github.com/CieriS/aria-er/actions/workflows/python.yml/badge.svg?branch=main)](https://github.com/CieriS/aria-er/actions/workflows/python.yml)
[![dbt](https://github.com/CieriS/aria-er/actions/workflows/dbt.yml/badge.svg?branch=main)](https://github.com/CieriS/aria-er/actions/workflows/dbt.yml)

End-to-end data platform for air quality in Emilia-Romagna, Italy, built on
[ARPAE](https://www.arpae.it) open data and enriched with weather data from Open-Meteo.

## Why

ARPAE publishes air quality measurements across dozens of monitoring stations,
but the data is split across yearly files and APIs, and recent measurements are
provisional and revised after validation. aria-er turns this into a clean,
historized and tested dataset that answers questions like:

- How many days did each station exceed the legal PM10 limit this year?
- How much does traffic contribute compared to background pollution in the same area?
- How much of a pollution peak is explained by weather (wind, rain)?
- Is air quality actually improving over the years?

## What it demonstrates

- **Idempotent, incremental ingestion** written in Rust, handling late-arriving
  and revised data through windowed reprocessing and upserts
- **Layered modeling** with dbt (staging → intermediate → marts), including
  SCD Type 2 history for monitoring stations
- **Data quality as code**: uniqueness, completeness and freshness tests
- **Orchestration** with Dagster: schedules, backfills, asset lineage
- **Reproducible infrastructure**: Docker Compose locally, Terraform on GCP
- **CI** on every pull request across Rust, Python and dbt

## Status

🚧 Work in progress — phases 0 to 6 are complete (exploration, Rust ingestor, dbt models,
Dagster orchestration, Open-Meteo weather, Streamlit dashboard, containers and CI). See the
roadmap below.

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

`make ingest` runs `aq-ingest run` and then `aq-ingest weather`, both taking
`[--from YYYY-MM-DD] [--to YYYY-MM-DD]`. The first downloads the station registry and the
measurements of the window, the second the hourly weather at the station coordinates:

```
raw/arpae/measurements/year=YYYY/month=MM/part-0.parquet
raw/arpae/stations/extracted_on=YYYY-MM-DD/stations.parquet
raw/arpae/station_types/extracted_on=YYYY-MM-DD/station_types.parquet
raw/openmeteo/weather/year=YYYY/month=MM/part-0.parquet
```

A full month for the whole region is ~113k rows and takes about 4 minutes, almost all of
it waiting for the ARPAE API (~40 s per request).

Configuration lives in [ingestor/config/default.toml](ingestor/config/default.toml)
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

## Architecture (target)

```
ARPAE API/CSV ─┐
               ├─► ingestor (Rust) ─► raw/ (Parquet, partitioned year/month)
Open-Meteo ────┘                          │
                                          ▼
                              DuckDB (local) | BigQuery (cloud)
                                          │
                                   dbt: staging ─► intermediate ─► marts
                                          │
                          Dagster (schedule, backfill, asset lineage)
                                          │
                                 Streamlit dashboard
```

Only the components of the current phase exist in the repository. The rest is the plan.

## Data sources

All sources come from the ARPAE CKAN portal, dataset
[`qualita-dell-aria-rete-di-monitoraggio`](https://dati.arpae.it/dataset/qualita-dell-aria-rete-di-monitoraggio).

| Source | Access | Coverage |
|---|---|---|
| Near-real-time measurements | CKAN datastore, resource `4dc855a1-6298-4b71-a1ae-d80693d43dcb`, via `datastore_search_sql` | Last ~7 weeks, with a provisional/validated flag |
| Historical measurements | Google Drive folder, one CSV per year × station × pollutant | 2010–2025, validated |
| Station registry | Google Sheets CSV export | Current stations and their pollutants |
| Pollutant registry | Google Sheets CSV export | IDs, units, averaging period |

Full details, schema and data quality findings: [docs/data-exploration.md](docs/data-exploration.md).

## Data quality findings and how they are handled

Phase 0 analysed one year (2025) of data for the Bologna stations. Each issue maps to a design decision:

| Issue found | Handling |
|---|---|
| **Missing hours are absent rows**, not nulls (coverage 94–99.5%, gaps up to 4+ days) | Never fill in raw. Build an expected time grid (station × pollutant × hour/day) and compare it with the actual data to compute completeness in `mart_data_completeness`. Aggregates (e.g. daily means) are only valid above a minimum coverage threshold (75% by EU convention). |
| **Provisional vs validated data** (`v_flag` `M`/`G` in the near-real-time feed, meaning undocumented) | Keep the flag in raw. Reprocess a rolling window (default 30 days) on every run and upsert by natural key `(station_id, pollutant, period_start)`, so revised values replace provisional ones. Validated history overrides near-real-time data for the same key. |
| **Inconsistent formats**: station code as `7000014`, `07000014` or `7.000.014`; dates as `DD/MM` vs `MM/DD`; CO in mg/m³ | Parse each source with its explicit format and fail loudly on mismatch. Normalise the station code to a single canonical integer in staging. Convert units to µg/m³ in staging, keeping the original value and unit in raw. |
| **Many exact zeros** (e.g. 242 hourly NO values at one station) | Likely below the detection limit or rounding. Keep the values and add a quality flag rather than dropping them silently, so the analyses can decide how to treat them. |
| **Timezone not declared**: timestamps do not follow daylight saving (24 hours on DST days) | Treated as fixed-offset local standard time (UTC+1) pending confirmation from ARPAE. Converted to UTC in storage. Europe/Rome is used only for presentation. |

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

On a fresh clone the volumes are empty and the dashboard says so. To load data, open
Dagster, choose *Materialize all* on the asset graph and pick a range of days (or wait for
the 06:00 schedule): ingestion, dbt and the checks run inside the container, and the
dashboard shows the result. A one-off ingestion without Dagster is also available:

```bash
docker compose run --rm ingestor run --from 2026-08-01 --to 2026-08-31
```

- **Images** are multi-stage, in `docker/`. The ingestor image is the Rust binary on a
  distroless base (67 MB); the two Python images install their locked dependencies with uv.
- **Volumes**: `raw` (Parquet written by the ingestor), `warehouse` (the DuckDB file,
  written by dbt and read by the dashboard) and `dagster_home` (run history).
- All containers run as the same unprivileged user, so they can share the volumes.

## Transformations (dbt on DuckDB)

`make transform` builds the warehouse in `warehouse/aria_er.duckdb` from the Parquet files
in `raw/` and the archive samples in `data/samples/`. It needs `make ingest` to have run at
least once.

| Layer | Model | Content |
|---|---|---|
| staging | `stg_arpae__measurements` | Near-real-time rows: typed, UTC timestamp, value in µg/m³ |
| staging | `stg_arpae__measurements_historical` | Validated archive CSVs aligned to the same key |
| staging | `stg_arpae__stations` | Latest extraction of the station registry |
| staging | `stg_arpae__station_types` | Traffic or background, and area type, from the ARPAE daily bulletin |
| snapshot | `snap_arpae__stations` | Type 2 history of the registry |
| intermediate | `int_measurements_deduplicated` | Incremental fact, one row per natural key, with quality flags |
| intermediate | `int_measurements_daily` | Daily mean, maximum and coverage |
| intermediate | `int_o3_8h_rolling` | Rolling 8-hour ozone mean |
| intermediate | `int_stations_current` | Current attributes of each station |
| mart | `mart_exceedances_yearly` | Exceedances of each legal limit per station and year |
| mart | `mart_data_completeness` | Coverage per station, pollutant and day, including empty days |
| staging | `stg_openmeteo__weather` | Hourly weather per location, UTC timestamp |
| intermediate | `int_weather_daily` | Daily wind, precipitation, temperature and pressure per location |
| mart | `mart_weather_correlation` | PM10 vs wind and rain, per station and season |
| mart | `mart_pollutant_trend` | Annual mean per municipality, pollutant and year |
| mart | `mart_traffic_vs_background` | Monthly traffic vs background means in the same municipality |

Seeds: `air_quality_limits` (legal thresholds) and `arpae_pollutants` (averaging period and
plausibility bound per pollutant). The station type is not a seed: it is ingested from the
ARPAE daily bulletin.

Rules applied:

- **Legal day.** Days and years are counted in ARPAE local standard time (UTC+1 all year).
  An hourly value stamped at the end of its hour belongs to the day the hour starts in.
- **Minimum coverage 75%.** A daily mean needs 18 of 24 hours, an 8-hour mean 6 of 8 hours,
  a daily 8-hour maximum 18 of 24 windows. Below that the period is not counted.
- **Exceedance** means strictly greater than the limit.
- **Flag, don't drop.** Negative, zero and implausible values stay in the fact table with
  `is_negative`, `is_zero`, `is_implausible`; only `is_valid` rows enter the aggregates.
  Two tests with `warn` severity surface them without failing the build.
- **Incremental lookback of 30 days**, the same window the ingestor reprocesses.
- **Freshness**: `dbt source freshness` warns when the newest measurement is older than 48 hours.

### Check against the ARPAE annual report (Bologna, 2025)

`mart_exceedances_yearly` compared with ARPAE's
[report on the 2025 data of the Bologna network](https://www.arpae.it/it/il-territorio/bologna/report-a-bo/aria/report-annuali-aria-bo):

| Indicator (2025) | Station | aria-er | ARPAE report |
|---|---|---|---|
| PM10 days above 50 µg/m³ | Porta San Felice | 20 | 20 |
| PM10 days above 50 µg/m³ | Giardini Margherita | 10 | 10 |
| PM10 days above 50 µg/m³ | Via Chiarini | 7 | 7 |
| O3 days with 8-hour maximum above 120 µg/m³ | Giardini Margherita | 27 | 27 |
| O3 days with 8-hour maximum above 120 µg/m³ | Via Chiarini | 54 | 54 |
| NO2 hours above 200 µg/m³ | all three | 0 | 0 |

The same data also reproduce figures the marts do not expose yet: NO2 annual means
(31 / 14 / 16 µg/m³), PM10 annual means (24 / 21 / 17 µg/m³) and ozone hours above
180 µg/m³ (2 at Giardini Margherita, 22 at Via Chiarini).

Notes on the comparison:

- PM10 figures are those of the report's ten-year table. Its monthly table for 2025 lists
  7 for Giardini Margherita and 10 for Chiarini, the opposite of its own ten-year table;
  the data agree with the ten-year table.
- The report states that its times are in standard time, which supports the UTC+1 assumption.
- For ozone the law caps the *three-year average* of exceedance days at 25. The mart
  compares each single year with 25, so `is_over_allowed_exceedances` is an approximation
  for ozone until three full years are loaded.
- 2026 rows cover August only (`year_coverage` ≈ 0.08): their counts are partial.

### Ten years of PM10 in Bologna

With the 2016–2024 archive files for PM10, PM2.5, NO2 and O3 of the three Bologna stations,
the marts reproduce the ten-year tables of the same ARPAE report:

- **Annual means** (`mart_pollutant_trend`, highest station): 26, 29, 26, 26, 26, 26, 27, 22,
  25, 24 µg/m³ for 2016–2025, identical to the report's Porta San Felice row.
- **Exceedance days** (`mart_exceedances_yearly`): Porta San Felice matches every year
  (33, 40, 18, 32, 42, 29, 33, 4, 26, 20), and so do Giardini Margherita and Via Chiarini.
  For 2023 the ten-year table of the 2025 report lists 6 and 3 for these two stations,
  while the marts give 3 and 5: the 2023 and 2024 editions of the same report both say 3
  and 5, month by month as in the marts, so the 2025 table has those two cells wrong.
- **NO2 annual means** (`mart_pollutant_trend`, highest station): 52, 46, 49, 46, 38, 44, 39,
  43, 28, 31 µg/m³, the report's Porta San Felice row except 2021 (44 against 43).
- **Traffic vs background** (`mart_traffic_vs_background`, Bologna, 2025): NO2 is on average
  31 µg/m³ at the traffic station against 15 at the background ones, PM10 24 against 19,
  while PM2.5 shows no difference (13.3 against 13.5). Across the nine municipalities with
  both kinds of station, the 2025 NO2 surplus at traffic stations ranges from 5.5 µg/m³
  (Ravenna) to 15.8 (Bologna); for PM10 the regional averages are 25.4 against 22.4.
- **Regional check**: for 2025 the mart finds a single station above the 35 allowed PM10
  exceedance days, Modena – Giardini with 40, as stated in ARPAE's regional summary for 2025.

### Legal limits to verify

The thresholds in `transform/seeds/air_quality_limits.csv` are consistent with the
values quoted in the ARPAE report, but have not been checked line by line against the text of
D.Lgs. 155/2010 in force:

| Pollutant | Metric | Limit | Allowed per year |
|---|---|---|---|
| PM10 | daily mean | 50 µg/m³ | 35 days |
| NO2 | hourly mean | 200 µg/m³ | 18 hours |
| O3 | daily maximum of the 8-hour rolling mean | 120 µg/m³ | 25 days, as a 3-year average (target value, not a limit value) |

Annual-mean limits (PM10 and NO2 40 µg/m³, PM2.5 25 µg/m³) are not in the seed because no
mart uses them yet. Directive (EU) 2024/2881 sets stricter values from 2030.

## Weather (Open-Meteo)

Hourly temperature, precipitation, wind speed and direction and surface pressure come from
the [Open-Meteo archive API](https://open-meteo.com/en/docs/historical-weather-api)
(`archive-api.open-meteo.com/v1/archive`), a reanalysis at roughly 10 km, queried in UTC.

- **Where.** At the coordinates of the ARPAE stations, rounded to one decimal (~11 km):
  the 54 stations of the registry collapse to 41 weather locations. A station is matched
  to its location by the same rounding in dbt (`weather_location_id`).
- **Storage.** Same partitioning and upsert as the measurements; natural key
  `(location_id, observed_at)`. Re-running a window leaves the files byte-identical.
- **Daily aggregation.** Days follow ARPAE local standard time with the same hour-ending
  convention as the pollutants, so a PM10 day and its weather day cover the same hours.

### Observed result: wind and PM10 in Bologna

`mart_weather_correlation`, three Bologna stations, 2025 plus August 2026:

| Station | Season | Days | Correlation PM10–wind | Mean PM10, windy days | Mean PM10, other days |
|---|---|---|---|---|---|
| Porta San Felice | winter | 90 | −0.47 | 11.0 µg/m³ (4 days) | 38.0 µg/m³ |
| Giardini Margherita | winter | 81 | −0.42 | 14.2 µg/m³ (5 days) | 34.0 µg/m³ |
| Via Chiarini | winter | 89 | −0.47 | 6.8 µg/m³ (4 days) | 29.3 µg/m³ |
| Porta San Felice | summer | 120 | −0.21 | 15.5 µg/m³ (12 days) | 20.4 µg/m³ |

A windy day is a day with mean wind of at least 3 m/s.

- **Wind**: the correlation is negative in every season at all three stations (−0.17 to
  −0.47) and strongest in winter, when calm, stable air lets particulate accumulate: on
  the few windy winter days PM10 is a third or less of the other days.
- **Rain**: the relation is much weaker (correlation between −0.28 and +0.07). Daily
  rainfall alone says little; one winter series even has slightly higher PM10 on rainy days.
- **Across the region** (42 stations with 2025 data): the winter PM10–wind correlation
  is negative at 41 of them, −0.39 on average, and mean PM10 is 14 µg/m³ on windy winter
  days against 33 on the others. In summer the link weakens (−0.11 on average, negative
  at 33 stations).
- Correlations are plain Pearson coefficients on daily values and say nothing about causes.

## Dashboard (Streamlit)

`make dashboard` opens the dashboard on the marts built by `make transform`. Each page
answers one of the questions at the top of this README:

| Page | Question | Mart |
|---|---|---|
| Exceedances | How many times did each station exceed the legal limits, per year? The legal maximum is a red line. | `mart_exceedances_yearly` |
| Trend | Is air quality improving over the years, by pollutant and city? | `mart_pollutant_trend` |
| Traffic vs background | How much does traffic add to the background pollution of the same city? | `mart_traffic_vs_background` |
| Weather and PM10 | How much of a PM10 peak is explained by wind and rain? | `mart_weather_correlation` |
| Data completeness | How complete are the data of each station, day by day? | `mart_data_completeness` |

Filters (stations, pollutant, municipality, period) are in the sidebar of each page.

The dashboard reads marts only and computes nothing: `data.py` holds one filtered `select`
per mart, `charts.py` turns a mart DataFrame into an Altair chart, `views.py` wires widgets
to the two. Thresholds, means and correlations all come from dbt. The warehouse is opened
read-only; its path can be changed with `AQ_DUCKDB_PATH`.

## Orchestration (Dagster)

`make orchestrate` starts the Dagster UI and its daemon. The asset graph is the whole
pipeline: the two raw assets written by `aq-ingest`, then every dbt seed, snapshot and model
downstream of them.

- **Ingestion assets** (`arpae_raw/measurements`, `arpae_raw/stations`,
  `arpae_raw/station_types`, `openmeteo_raw/weather`): run the `aq-ingest` binary (`run` and `weather` subcommands)
  for the selected days. No ingestion logic is duplicated in Python. The counters
  of the run (fetched, inserted, updated rows) are attached to the materialization.
- **dbt assets**: loaded from the dbt manifest with `dagster-dbt`; dbt sources and the raw
  assets share the same keys, which is what connects the lineage. dbt tests appear as asset checks.
- **Daily partitions** on every asset, in ARPAE local standard time. A range of days runs as
  a *single* run (`aq-ingest --from … --to …`, then one `dbt build`), from the UI
  (Materialize → select a range) or with `make backfill`.
- **Schedule** `reprocess_provisional_window`: every day at 06:00 Europe/Rome it re-ingests
  the last 30 days, the window in which ARPAE may still revise data, and rebuilds the models.
  It is on by default; it only fires while the daemon (`make orchestrate`) is running.
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

## Design decisions (phase 1)

- **Upsert by rewriting partitions** ([ADR 0001](docs/adr/0001-parquet-upsert-by-partition-rewrite.md)).
  Each run merges the incoming rows into the monthly files it touches, by natural key,
  and rewrites them sorted. Re-running a window yields byte-identical files and never
  duplicates; a revised value replaces the provisional one.
- **Near-real-time datastore as the only source for now**
  ([ADR 0002](docs/adr/0002-near-real-time-datastore-as-phase-1-source.md)). One SQL query
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

## Design decisions (phase 2)

- **Archive read by dbt for now** ([ADR 0003](docs/adr/0003-historical-archive-read-by-dbt.md)).
  Yearly counts need a full year, which only the validated archive has. Until the ingestor
  can backfill it, a staging model reads the sample CSVs directly; the archive wins over
  the near-real-time feed for the same key.
- **One incremental fact, `delete+insert` on the natural key.** Aggregates on top are
  plain tables: at this volume a full rebuild takes under a second, and incremental
  aggregates would have to handle late revisions.
- **No dbt packages.** The only helper needed, a multi-column uniqueness test, is a
  five-line generic test in the project.
- **Limits and pollutant metadata as seeds.** No threshold, pollutant id or averaging
  period is written in a model; the ozone model selects its pollutant from the seed.
- **Completeness against an expected grid.** Missing hours are absent rows, so coverage
  is computed against every day between the first and last observation of a series.

## Design decisions (phase 3)

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

## Design decisions (phase 4)

- **`Source` and `Sink` generic over the record type**
  ([ADR 0004](docs/adr/0004-generic-source-and-sink-traits.md)). The traits were shaped
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

## Design decisions (phase 5)

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

## Design decisions (phase 6)

- **One Dagster container** running `dagster dev` (webserver and daemon together) instead of
  separate services with a Postgres run storage: enough for a single-machine stack.
- **The orchestration image carries the binary and the dbt project**, because Dagster calls
  both. The ingestor image exists on its own for one-off runs and as the minimal artifact.
- **Named volumes, not bind mounts**, so a clone needs no local directories or permissions.
- **dbt in CI runs on committed fixtures** (a 76 KB real slice written with the ingestor's
  schema), built twice to cover the incremental path, with no network.

## Planned marts

| Mart | Question |
|---|---|
| `mart_exceedances_yearly` | How many times each station exceeded legal limits, per year |
| `mart_pollutant_trend` | Multi-year trend by pollutant and city |
| `mart_traffic_vs_background` | Traffic vs background stations in the same area |
| `mart_weather_correlation` | Wind/rain vs PM10 peaks |
| `mart_data_completeness` | Data coverage per station (quality audit) |

Legal limits (Italian D.Lgs. 155/2010) will live in a dbt seed, never hardcoded.

## Continuous integration

Three workflows run on every pull request and on every push to `main`, each with its
dependency cache:

| Workflow | Checks |
|---|---|
| [Rust](.github/workflows/rust.yml) | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` |
| [Python](.github/workflows/python.yml) | `ruff format --check`, `ruff check`, `mypy --strict`, `pytest`, for `orchestration` and `dashboard` |
| [dbt](.github/workflows/dbt.yml) | `dbt build` on DuckDB with the committed fixtures, twice (full and incremental) |

No workflow calls ARPAE or Open-Meteo: sources are mocked behind traits in Rust, and dbt
reads `transform/fixtures/`. `make transform-fixtures` runs the dbt job locally. Actions are
pinned by commit, and Dependabot proposes weekly updates for actions, crates, Python
packages and base images.

### Branch protection

`main` only changes through pull requests that pass the CI. The rule to set under
*Settings → Rules → Rulesets* (target: default branch):

- **Require a pull request before merging** (no direct pushes to `main`).
- **Require status checks to pass**, with the branch up to date, for these checks:
  `fmt, clippy, test`, `orchestration (ruff, mypy, pytest)`, `dashboard (ruff, mypy, pytest)`
  and `dbt build on fixtures (DuckDB)`.
- **Block force pushes** and branch deletion.

Approvals are not required: this is a single-maintainer repository, so the gate is the CI.

## Roadmap

- [x] **0. Exploration** — one year of Bologna data, schema and quality issues documented
- [x] **1. Minimal Rust ingestor** — ARPAE → partitioned Parquet, idempotent, tested
- [x] **2. dbt on DuckDB** — staging + `mart_exceedances_yearly` with tests
- [x] **3. Orchestration** — Dagster daily schedule and backfills
- [x] **4. Second source** — Open-Meteo + `mart_weather_correlation`
- [x] **5. Dashboard** — Streamlit
- [x] **6. Full CI** + Docker Compose
- [ ] **7. Cloud** — GCS + BigQuery via Terraform, same dbt models with a different target
- [ ] **8. (Optional) Streaming** — only if justified in an ADR

A phase is done when it works end-to-end, has tests, and this README is updated.

## Repository layout (current)

```
data/samples/arpae/   raw samples downloaded in phase 0 (source of the test fixtures)
docs/                 data exploration report, ADRs
ingestor/             Rust workspace
  config/             default configuration
  crates/core/        domain models, Source/Sink traits, errors
  crates/http/        HTTP transport with timeouts and retry
  crates/source-arpae/  ARPAE client and parsing
  crates/source-openmeteo/  Open-Meteo archive client and parsing
  crates/sink-parquet/  partitioned Parquet writer with upsert
  crates/cli/         aq-ingest binary
transform/            dbt project (DuckDB): staging, intermediate, marts, seeds, snapshot, tests
orchestration/        Dagster project: assets, checks, schedule, resources, tests
dashboard/            Streamlit app: data access, charts, pages, tests
scratch/              throwaway exploration scripts
docker/               Dockerfiles for ingestor, orchestration and dashboard
docker-compose.yml    local stack
Makefile              ingest / transform / orchestrate / backfill / dashboard / up / down / test / lint
```

## Known limitations

- **No historical backfill yet.** Only the near-real-time datastore (last ~7 weeks) is ingested. The
  historical archive is hosted on Google Drive without a stable API (the file list is an HTML page);
  a window older than the datastore retention simply returns no rows.
- **The source can be stale.** On 2026-10-01 the newest measurement in the datastore was dated
  2026-09-17. The ingestor does not alert on freshness yet.
- Rows deleted upstream are not removed from the raw layer (upsert only), and previous values of
  revised rows are not kept.
- **Archive data is a sample**, read from CSVs in the repository: 2025 PM10, PM2.5 and NO2 for the
  46 traffic and background stations of the region; every pollutant for 2025 and PM10, PM2.5, NO2
  and O3 for 2016–2024 for the three Bologna stations. 2026 covers August only.
  January–July 2026 is missing from every source and shows as empty days in `mart_data_completeness`.
- Archive files older than the 30-day lookback need `dbt build --full-refresh` to be loaded.
- An 8-hour window ending on a missing hour is not produced; days with many gaps may lack a few windows.
- Station types follow the latest ARPAE bulletin; their history is kept in raw (one snapshot per
  day) but not yet modelled as a type 2 dimension.
- The registry snapshot has few extractions so far, so little history yet.
- dbt tests run against local data, not fixtures; `make test` therefore needs `raw/`.
- The schedule and UI backfills need the local daemon running; nothing runs when the machine is off.
- A range of days is all-or-nothing: if the run fails, every partition in it is marked failed.
- The registry snapshot is shown as daily-partitioned in Dagster although it is a dated extraction.
- Partitions older than the datastore retention (~7 weeks) materialize successfully but ingest no rows.
- Weather is a ~10 km reanalysis, not a measurement at the station, and stations within the same
  rounded cell share identical weather.
- Open-Meteo rate-limits the free API: a year for all locations right before another request can
  answer 429 beyond the ingestor's retries (the Dagster retry, one minute later, covers it).
- The rounding precision is configured twice (ingestor config and dbt variable) and must match.
- DuckDB allows one writer: while a dbt build is running the dashboard waits up to 15 seconds
  for it, then asks to reload.
- Dashboard texts are in English and dates are shown as stored (calendar days in ARPAE standard time).
- The Python images are large (about 0.9 GB each): Dagster, dbt and Streamlit pull in heavy
  dependencies and no effort was made to slim them.
- The container stack keeps Dagster's run history in SQLite on a volume: fine for one machine,
  not for several workers.
- Single writer: two concurrent runs on the same partition would race.
- The ARPAE API is slow and intermittently returns 502; runs rely on retries with backoff.
- No source covers early 2026 at the moment: it is not yet in the historical archive and is already outside the near-real-time window.
- The meaning of `v_flag` and the timezone of the timestamps are inferred, not documented by ARPAE.

## License and data

Data © ARPAE Emilia-Romagna, published as open data. Check the license on the dataset page before reuse.
