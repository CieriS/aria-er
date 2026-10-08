# 0003 — Historical archive read by dbt until the ingestor supports backfill

Status: superseded by [0005](0005-archive-loaded-by-the-ingestor.md)

## Context

Yearly exceedance counts need full years of data, but the ingestor only loads the
near-real-time datastore (last ~7 weeks, ADR 0002). The validated archive is a set of
CSV files on Google Drive; one year for the Bologna stations was downloaded by hand in
phase 0 and lives in `data/samples/arpae/`.

## Options

1. **Add the archive to the Rust ingestor now.** The right long-term place, but it means
   scraping an HTML folder listing and reopening phase 1 in the middle of phase 2.
2. **Read the CSV files from dbt as a second source.** DuckDB reads them in place; a
   staging model aligns them to the key and conventions of the near-real-time feed.
3. **Build the marts on near-real-time data only.** No full year exists, so yearly
   figures could not be checked against ARPAE's published reports.

## Decision

Option 2. `stg_arpae__measurements_historical` parses the archive files and
`int_measurements_deduplicated` unions both sources by natural key, the archive winning
over the near-real-time feed.

## Consequences

- The marts cover 2025 only for the stations whose files are in `data/samples/`.
- CSV parsing lives in a staging model instead of the ingestor; when the backfill is
  implemented the archive becomes Parquet in `raw/` and the dbt source changes location,
  not shape.
- Archive files added for dates older than the incremental lookback are not picked up by
  an incremental run: they need `dbt build --full-refresh`.
