# 0002 — Near-real-time datastore as the only measurement source in phase 1

Status: accepted (phase 1)

## Context

ARPAE publishes measurements in two places (see `docs/data-exploration.md`):

- a CKAN datastore resource with the last ~7 weeks, carrying a validation flag;
- a Google Drive folder with one validated CSV per year × station × pollutant, listed
  only through an HTML page.

The incremental pipeline needs recent, revisable data. The historical backfill is a
separate concern with a fragile access path.

## Decision

Phase 1 ingests only the datastore resource, through `datastore_search_sql`.

- **Window filter.** `reftime` is text (`MM/DD/YYYY HH:MM`), so days are compared on
  `substr(reftime,7,4)||substr(reftime,1,2)||substr(reftime,4,2)`. One query covers the
  whole window instead of one query per day: each query takes ~40 s regardless of size.
- **Pagination.** Keyset on `_id` (`_id > last ORDER BY _id LIMIT n`), stopping only on
  an empty page, so a server-side cap lower than the requested page size cannot
  silently truncate the result.
- **Timestamps.** Published times do not follow daylight saving. They are treated as a
  fixed UTC+1 offset (configurable) and converted to UTC. `measured_at` is the published
  reference time as is: end of the hour for hourly pollutants, the day itself for daily
  ones. The original string is kept in `raw_reftime`, so the conversion can be redone
  if ARPAE documents a different convention.
- **Units.** The datastore has no unit column: the unit is joined from the pollutant
  registry at ingestion time and stored next to the value, without conversion.

## Consequences

- Dates older than the datastore retention return no rows; the historical archive is not
  ingested yet. The `Source` trait is the extension point for it.
- If the UTC+1 assumption is wrong, timestamps are off by a constant and can be
  recomputed from `raw_reftime` without downloading again.
- Rows that disappear upstream are not deleted from the raw layer (upsert only).

## Update, 2026-10-10

The UTC+1 assumption is confirmed: ARPAE's REST archive publishes timestamps with an explicit
`+01:00` offset all year. The datastore itself has not been updated since 2026-09-17; see
`docs/data.md`.
