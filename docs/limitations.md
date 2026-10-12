# Known limitations

What does not work, is approximate, or is still manual.

## Known limitations

- **The archive is loaded from local files, not downloaded.** It is hosted on Google Drive without a
  stable API (the file list is an HTML page), so new files are added to `data/samples/arpae/` by
  hand.
- **The source can be stale.** The CKAN datastore stopped on 2026-09-17 and was replaced; on
  2026-10-11 (a Sunday) the newest value of the REST archive was from 11:00 of Friday
  2026-10-09. Only `dbt source freshness` says so, as a warning after 48 hours.
- **Identifiers of the REST archive are mapped by hand.** `arpae.station_ids` and
  `arpae.pollutant_ids` in the ingestor configuration translate them to the registry codes.
  A new station is left out, with a warning, until it is added; ethylbenzene and m,p-xylene
  have no registry id and are not ingested.
- **The REST archive is not a superset of the datastore it replaced.** In August 2026 it has 23
  more stations for PM10 and NO2 (the local networks), but lacks NOx for 16 stations and NO for
  15 that the datastore published (Porta San Felice among them), toluene for 3 and o-xylene
  for 2. No legal limit is counted on those pollutants; their trends have fewer stations.
- Stations of the local networks are not in the station registry: they have a name and a
  province (from the type registry) but no municipality or coordinates, so they are in the
  exceedance and completeness marts and not in the per-city trend, the traffic/background
  comparison or the weather correlation.
- **The reference-time convention of the REST archive is inferred from data**, not documented
  by ARPAE: hourly values at the start of the hour, daily ones at the end of the day. It was
  checked against the yearly files, the CKAN datastore and the bulletins ([data.md](data.md)).
- Which pollutants are daily is configured twice (`arpae.daily_pollutant_ids` in the ingestor,
  `averaging_minutes` in the dbt seed) and must match.
- Rows deleted upstream are not removed from the raw layer (upsert only), and previous values of
  revised rows are not kept.
- **Archive data is a sample** of the files kept in the repository: 2025 PM10, PM2.5 and NO2 for the
  46 traffic and background stations of the region; every pollutant for 2025 and PM10, PM2.5, NO2
  and O3 for 2016–2024 for the three Bologna stations. 2026 comes from the REST archive for every
  station, once ingested (`make ingest FROM=2026-01-01`); a first start only loads the last 30 days.
- A *corrected* value in an archive file already loaded, older than the 30-day lookback, needs
  `dbt build --full-refresh` to reach the marts. New archive rows are picked up on their own.
- Station types come from the ARPAE station registry (`qa_stazioni`), snapshotted at every run, so a
  reclassification is picked up. If the registry stops stating the exposure, the last snapshot that
  did is used, then a reference seed frozen at 2026-10-04, and a `warn` test lists the stations
  affected. The raw column holding the version of the source record is still called `bulletin_id`.
- The history of station types is kept in raw (one snapshot per day) but not modelled as a type 2
  dimension.
- The registry snapshot has few extractions so far, so little history yet.
- dbt tests run against local data, not fixtures; `make test` therefore needs `raw/`.
- The schedule and UI backfills need the local daemon running; nothing runs when the machine is off.
- A range of days is all-or-nothing: if the run fails, every partition in it is marked failed.
- The registry snapshot is shown as daily-partitioned in Dagster although it is a dated extraction.
- Partitions start on 2026-01-01: earlier years of the REST archive (it goes back to late 2021)
  can be ingested with `make ingest FROM=... TO=...` but are not scheduled; the yearly files win
  over them for the same key anyway.
- Weather is a ~10 km reanalysis, not a measurement at the station, and stations within the same
  rounded cell share identical weather.
- Open-Meteo rate-limits the free API: a year for all locations right before another request can
  answer 429 beyond the ingestor's retries (the Dagster retry, one minute later, covers it).
- The rounding precision is configured twice (ingestor config and dbt variable) and must match.
- DuckDB allows one writer: while a dbt build is running the dashboard waits up to 15 seconds
  for it, then asks to reload.
- Dashboard texts are in English and dates are shown as stored (calendar days in ARPAE standard time).
- The Python images stay large (about 0.8 GB unpacked, under 0.2 GB to pull): most of it is
  pyarrow, pandas, DuckDB and Dagster themselves. Not precompiling bytecode saved about 10%.
- The container stack keeps Dagster's run history in SQLite on a volume: fine for one machine,
  not for several workers.
- Single writer: two concurrent runs on the same partition would race.
- The ARPAE API is slow and intermittently returns 502; runs rely on retries with backoff.
- **The cloud target has not been run on a real GCP project yet.** Terraform is validated, the
  storage backend is tested on an in-memory object store, and the BigQuery SQL is executed on an
  emulator with results identical to DuckDB. External tables, IAM and dbt's BigQuery
  materialisations are unverified. See [cloud.md](cloud.md).
- On BigQuery every build re-reads the Parquet files behind the external tables; with a much larger
  archive this would leave the free tier of read operations.
