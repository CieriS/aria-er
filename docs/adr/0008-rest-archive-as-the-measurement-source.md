# 0008 — Measurements come from the REST archive, not from the CKAN datastore

Status: accepted (2026-10-12), supersedes 0002

## Context

The CKAN datastore read since phase 1 ([ADR 0002](0002-near-real-time-datastore-as-phase-1-source.md))
has had no measurement after 2026-09-17. ARPAE publishes the same network on the archive of its
REST service, `https://apps.arpae.it/REST/qa_archivio_dati_public`: about 10.5 million records
from late 2021 onwards, with the offset in the timestamp, the unit in the record and the
validation stages passed so far.

It is the same data in a different shape. Checked on 2026-09-16, a day present in both sources,
and on February 2025 against the yearly files:

| | CKAN datastore | REST archive |
|---|---|---|
| Station id | registry code (`7000015`) | its own (`1103`) |
| Parameter id | registry id (NO2 = `8`) | its own (NO2 = `32`) |
| Hourly reference time | end of the hour | **start** of the hour |
| Daily reference time | end of the day | end of the day |
| Value | rounded (integers; one decimal for CO and benzene) | unrounded |
| Unit | not published | `µg/m³`, `mg/m³` |
| Validation | one letter (`G`, `M`) | stages passed (`Giornaliera + Mensile`) |

Once ids and hours are aligned, every value of the datastore equals the rounded value of the
archive, for the eleven parameters and the 41 stations the two have in common that day.

The check also showed that the datastore stamped daily values at the **end** of the day, like
the REST archive, and not on the day itself as the parameter registry says and as the pipeline
assumed: the PM10 the bulletin of 9 August reports for a station is the value the datastore
dated 10 August. PM10 and PM2.5 ingested from the datastore were therefore attributed to the
following day.

## Decision

`ArpaeSource` reads the REST archive. The datastore client, the pollutant registry download
(only used for units) and their configuration are removed.

- **Same port, same record, same raw dataset.** The source still implements
  `Source<Record = Measurement>` and writes `raw/arpae/measurements/`. Nothing downstream
  changes shape: dbt, Dagster and the dashboard are untouched apart from one macro that now
  also reads the unit spelled `µg/m³`.
- **Identifiers are translated by the adapter, from configuration.** `arpae.station_ids` and
  `arpae.pollutant_ids` in `config/default.toml` map the archive ids onto the registry ones
  the raw layer, the seeds and the yearly files already use. The station map comes from the
  daily bulletin, which lists both ids (two mobile laboratories it does not list were matched
  by name); it is a table in the repository rather than a lookup at run time, because the
  bulletin has already changed format once without notice.
- **Only mapped parameters are requested** (the filter is part of the query). A station
  missing from the map is left out and reported in a warning with its id.
- **Reference times are moved to the convention of the raw layer**: end of the hour for
  hourly values (published time + 1 h), the day itself for daily ones (published time − 24 h).
  Which pollutants are daily is configuration (`arpae.daily_pollutant_ids`). The published
  string stays in `raw_reftime`.
- **The window is the day the averaging period starts in.** One query covers from the first
  midnight of the window to the midnight after its last day, then rows of the neighbouring
  days are dropped: the daily value stamped at the first midnight, the hourly ones starting
  at the last.
- **Pagination by page number, sorted by `_id`**, until the total the server reports is
  reached, so a page size capped by the server cannot truncate the result.
- **Values are stored unrounded.** Rounding belongs to whoever compares with a limit:
  `mart_exceedances_yearly` now rounds to a whole number first, which is what makes the PM10
  exceedance days of 2026 equal to the ARPAE bulletin for all 64 stations it lists
  (`docs/transformations.md`).
- **Pages of 2000 records.** The server accepts 5000 and more, but cuts the connection in the
  middle of large responses often enough to exhaust the retries on a month of data.

## Alternatives considered

- **A separate raw dataset with ids as published, translated in dbt.** More faithful raw
  layer, and the mapping could be corrected without downloading again. It needs a new
  source, staging model, external table and fixtures for data that has the same meaning as
  the dataset it replaces; the yearly files already set the precedent of resolving the
  reference time in the adapter.
- **Keeping both clients.** The datastore is not updated and holds a subset of the archive.
  Git history has the client if ARPAE revives it.
- **Reading the station map from the bulletin at every run.** One more request that can
  fail, on a document whose fields have changed before.

## Consequences

- The raw layer can hold 2026 in full: the gap between the yearly files (up to 2025) and
  the seven weeks of the datastore is gone. Dagster partitions start on 2026-01-01.
- PM10 and PM2.5 of 2026 land on the right day. Rows ingested from the datastore before
  this change are not corrected in place: the raw measurements must be ingested again
  (see `docs/running.md`).
- Four things are configured by hand and can go stale: the two id maps, the list of daily
  pollutants (also in the dbt seed `arpae_pollutants`) and the page size.
- A new station appears as a warning, not as data, until its id is added to the map.
- Ethylbenzene and m,p-xylene, published by the archive, have no id in the parameter
  registry and are not ingested.
- The archive covers more stations (68 against 46: the local networks come in) but fewer
  series of NO and NOx: about fifteen stations that had them in the datastore do not in the
  archive. No limit is counted on them.
- Whether the archive is fed continuously is not known: all of it was loaded on 2026-10-09
  and it has not been written since (`docs/data.md`, update of 2026-10-12). It is still the
  only source with 2026 in full, so the decision holds for history; what the project shows
  as "recent" if the archive stays still is open (#40).
