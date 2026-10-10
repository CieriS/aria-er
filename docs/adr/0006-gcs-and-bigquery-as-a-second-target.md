# 0006 — Cloud Storage and BigQuery as a second target of the same pipeline

Status: accepted (phase 7); not yet exercised on a real project

## Context

Phase 7 asks for the pipeline on GCP inside the free tier, with the same dbt models as
locally. Three choices shape it.

## Decisions

**1. Storage is a backend of the existing sink, not a second sink.**
Options: a new `sink-gcs` crate (the upsert, ordering and atomic write duplicated), syncing
local files with `gcloud storage rsync` (the ingestor would not write to GCS at all), or a
`Storage` trait inside `sink-parquet`. Chosen: the trait, with a local implementation by
default and any `object_store` backend behind the Cargo feature `gcs`. One upsert, the same
bytes on disk and in a bucket, and the async runtime and HTTP client of `object_store` are
not compiled into the default build.

**2. BigQuery reads the raw layer through external tables.**
Options: load jobs into native tables after each ingestion, or external tables over the
Parquet files. Chosen: external tables, defined in Terraform with an explicit schema. The
bucket stays the single copy of the raw layer and dbt sources mean the same thing on both
targets. Cost: every query on a source reads the files again, which is what bounds the
free-tier use (see `docs/cloud.md`).

**3. One set of models, dialect differences in macros.**
The models are written in the SQL both engines accept (case expressions instead of
`FILTER`, `extract()` instead of `year()`, explicit `GROUP BY` columns, dbt's `datediff`
and `date_trunc`). What cannot
be shared is in `macros/dialect.sql`, dispatched by adapter: the naive-UTC timestamp type
(`TIMESTAMP` on DuckDB, `DATETIME` on BigQuery), date construction, a numeric key for
`RANGE` window frames, the date spine, and the incremental strategy (`delete+insert`
against `merge`).

## Consequences

- The rewrite changed no result on DuckDB: every mart and intermediate model was compared
  row by row before and after.
- Nothing runs permanently in the cloud; there is no compute to pay for.
- Until the first run on a real project the BigQuery side is checked on the open-source
  BigQuery emulator (`make bigquery-dialect-check`): every model is analysed and executed,
  and the results match DuckDB on the fixtures. That check found what a syntax parser had
  not: a numeric column compared with quoted values, `GROUP BY ALL` (replaced by explicit
  columns, which every engine accepts) and a table alias shadowed by a column alias in
  `GROUP BY`. `make cloud-compare` on real data remains the acceptance test.
- External tables need files to exist, hence the two Terraform passes.
- Region `us-central1`: the only way to stay in the Cloud Storage free tier.
