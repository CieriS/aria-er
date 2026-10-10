# Known limitations

What does not work, is approximate, or is still manual.

## Known limitations

- **The archive is loaded from local files, not downloaded.** It is hosted on Google Drive without a
  stable API (the file list is an HTML page), so new files are added to `data/samples/arpae/` by
  hand. The near-real-time datastore only holds the last ~7 weeks.
- **The source can be stale.** On 2026-10-01 the newest measurement in the datastore was dated
  2026-09-17. The ingestor does not alert on freshness yet.
- Rows deleted upstream are not removed from the raw layer (upsert only), and previous values of
  revised rows are not kept.
- **Archive data is a sample** of the files kept in the repository: 2025 PM10, PM2.5 and NO2 for the
  46 traffic and background stations of the region; every pollutant for 2025 and PM10, PM2.5, NO2
  and O3 for 2016–2024 for the three Bologna stations. 2026 covers August only.
  January–July 2026 is missing from every source and shows as empty days in `mart_data_completeness`.
- Archive rows older than the 30-day lookback need `dbt build --full-refresh` to reach the marts.
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
- The Python images stay large (about 0.8 GB unpacked, under 0.2 GB to pull): most of it is
  pyarrow, pandas, DuckDB and Dagster themselves. Not precompiling bytecode saved about 10%.
- The container stack keeps Dagster's run history in SQLite on a volume: fine for one machine,
  not for several workers.
- Single writer: two concurrent runs on the same partition would race.
- The ARPAE API is slow and intermittently returns 502; runs rely on retries with backoff.
- No source covers early 2026 at the moment: it is not yet in the historical archive and is already outside the near-real-time window.
- The meaning of `v_flag` and the timezone of the timestamps are inferred, not documented by ARPAE.
