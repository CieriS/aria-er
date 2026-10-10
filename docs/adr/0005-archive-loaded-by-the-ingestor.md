# 0005 — The ARPAE archive is loaded by the ingestor

Status: accepted (phase 6), supersedes 0003

## Context

ADR 0003 let dbt read the yearly archive CSVs directly, as a shortcut to get full years
into the marts. It left two flaws: CSV parsing and timestamp conventions lived in a
staging model instead of the ingestor, and the raw layer did not contain everything the
marts are built from.

## Decision

A third `Source<Record = Measurement>` in `source-arpae`, `ArpaeArchive`, reads the archive
files (`storico_<year>/*.csv[.gz]`) from a local directory. `aq-ingest archive` writes them
to `raw/arpae/measurements_archive/` with the same partitioning and upsert as the
near-real-time measurements.

- **A separate dataset, not the same files as the near-real-time feed.** Both describe
  the same keys around the turn of the year; keeping them apart lets dbt apply the
  precedence rule (the validated archive wins) instead of "last writer wins".
- **Same record type.** `validation_flag` became optional: the archive is validated and
  has no flag.
- **Files are read from disk, not downloaded.** The archive is published as a Google Drive
  folder listed only through an HTML page; fetching it stays a manual step.
- **In Dagster the archive is one unpartitioned asset with its own job.** It is a fixed set
  of files; the upsert makes reloading it a no-op when nothing changed.

## Consequences

- dbt staging for the archive is a plain typed select of Parquet, identical in shape to the
  near-real-time one; the `historical_dir` variable and the CSV fixtures are gone.
- The whole lineage, archive included, is visible in Dagster.
- Getting new archive files into `data/samples/` is still manual.
- The incremental model lets archive rows it has not loaded yet through, whatever their age,
  so adding a year of files needs no full refresh. Only a correction to rows already loaded
  and older than the lookback does.
