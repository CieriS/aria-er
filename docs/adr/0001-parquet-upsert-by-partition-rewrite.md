# 0001 — Upsert on Parquet by rewriting whole partitions

Status: accepted (phase 1)

## Context

ARPAE revises recent measurements, so every run re-ingests a rolling window and must
replace rows by natural key `(station_id, pollutant_id, measured_at)`. Parquet files are
immutable: there is no in-place update. Re-running the same window must not create
duplicates and should produce the same bytes.

## Options

1. **Append a new file per run, deduplicate at read time.** Cheapest write, but the raw
   layer then contains duplicates, every reader must know the dedup rule, and files
   accumulate without compaction.
2. **Table format with merge support (Delta Lake, Iceberg).** Proper upserts and time
   travel, but a heavy dependency and a metadata layer for a dataset of ~110k rows per
   month.
3. **Rewrite the touched partitions.** Read the existing partition file, merge with the
   incoming rows by key (incoming wins), sort by key, write a single file to a temporary
   path and rename it over the old one.

## Decision

Option 3. One file per `year=YYYY/month=MM` partition (UTC), fully rewritten on upsert.

- No duplicates can exist in the raw layer, by construction.
- Rows are sorted by key and nothing run-dependent (ingestion time, run id) is stored, so
  the same data yields a byte-identical file regardless of input order or batching.
- A partition whose content does not change is not rewritten.
- The rename makes each partition switch atomically; a crash leaves the old file intact.

## Consequences

- Write cost is proportional to the partition size, not to the number of changed rows.
  Fine at this volume (a monthly partition is ~1 MB); it would need revisiting for a much
  larger grain.
- A run touching several partitions is atomic per partition, not as a whole. Since the
  operation is idempotent, the recovery is to run it again.
- Previous values of revised rows are not kept. If revision history becomes a
  requirement it has to be modelled explicitly.
- Single writer assumed: two concurrent runs on the same partition would race.
