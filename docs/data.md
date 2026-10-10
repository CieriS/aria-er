# Data sources and quality

Where the data comes from and how its problems are handled. The full exploration report is in [data-exploration.md](data-exploration.md).

## Data sources

Measurements and registries come from the ARPAE CKAN portal, dataset
[`qualita-dell-aria-rete-di-monitoraggio`](https://dati.arpae.it/dataset/qualita-dell-aria-rete-di-monitoraggio);
station types and the newer archive from the ARPAE REST service (`https://apps.arpae.it/REST/`).

| Source | Access | Coverage |
|---|---|---|
| Near-real-time measurements | CKAN datastore, resource `4dc855a1-6298-4b71-a1ae-d80693d43dcb`, via `datastore_search_sql` | Last ~7 weeks, with a validation flag. **Not updated since 2026-09-17** |
| Historical measurements | Google Drive folder, one CSV per year × station × pollutant | 2010–2025, validated |
| Station registry | Google Sheets CSV export | Current stations and their pollutants |
| Pollutant registry | Google Sheets CSV export | IDs, units, averaging period |
| Station types | REST `qa_stazioni` | 77 stations: exposure (`tipo_stazione`), area (`zona`), coordinates, network |
| Measurement archive (**not ingested yet**) | REST `qa_archivio_dati_public` | About 10.5 million records from late 2021 to a few hours ago, with timezone, validation level and publication time |

Full details, schema and data quality findings: [docs/data-exploration.md](data-exploration.md).

## Validation flag

The datastore documents `v_flag` in its resource description: `T` temporary (automatic
check), `G` validated daily, `M` validated monthly, `S` validated six-monthly. Recent days
are `G` and older months `M`: a higher level means *more* validated, and a value can be
revised at each stage. The flag is stored as published and no model depends on its meaning.

## Timezone

Timestamps are in fixed UTC+1 all year. This was first inferred (24 hours on daylight
saving days) and is now confirmed by ARPAE itself: the REST archive publishes
`2026-10-09T11:00:00+01:00` in October, when civil time in Italy is UTC+2.

## Upstream changes observed

- **2026-09-17, CKAN datastore**: the newest measurement has been dated 2026-09-17 since at least
  1 October. The resource description says it is updated daily; it is not being updated.
- **2026-10-09, REST archive**: `qa_archivio_dati_public` appeared, with fresh hourly values
  published in batches a few hours after measurement. It uses new station and parameter
  identifiers (Porta San Felice is `1103`, NO2 is `32`; the bulletin maps them through
  `originalidstazione`), unrounded values, and stamps a daily value at the *end* of its day
  (the PM10 of 23 February 2025 is at `2025-02-24T00:00:00+01:00`), unlike the datastore and the
  yearly files. Taken together with the bulletin change below, ARPAE appears to be moving to a new
  platform.

- **2026-10-06, daily bulletin**: `tipostazione` went from `Urbana Traffico` / `Urbana Fondo` to
  the area alone (`Urbana`), and the records gained `locality` and `originalidstazione`. Nothing
  failed: the ingestor stores the label as published, and the classification downstream silently
  became "unknown" for every station. The station type is now ingested from the station registry
  `https://apps.arpae.it/REST/qa_stazioni` (`tipo_stazione`, `zona`), which also classifies the
  industrial stations the bulletin labelled as local.

## Data quality findings and how they are handled

Phase 0 analysed one year (2025) of data for the Bologna stations. Each issue maps to a design decision:

| Issue found | Handling |
|---|---|
| **Missing hours are absent rows**, not nulls (coverage 94–99.5%, gaps up to 4+ days) | Never fill in raw. Build an expected time grid (station × pollutant × hour/day) and compare it with the actual data to compute completeness in `mart_data_completeness`. Aggregates (e.g. daily means) are only valid above a minimum coverage threshold (75% by EU convention). |
| **Data revised after publication** (`v_flag`: temporary, then validated daily, monthly, six-monthly) | Keep the flag in raw. Reprocess a rolling window (default 30 days) on every run and upsert by natural key `(station_id, pollutant, period_start)`, so revised values replace provisional ones. Validated history overrides near-real-time data for the same key. |
| **Inconsistent formats**: station code as `7000014`, `07000014` or `7.000.014`; dates as `DD/MM` vs `MM/DD`; CO in mg/m³ | Parse each source with its explicit format and fail loudly on mismatch. Normalise the station code to a single canonical integer in staging. Convert units to µg/m³ in staging, keeping the original value and unit in raw. |
| **Many exact zeros** (e.g. 242 hourly NO values at one station) | Likely below the detection limit or rounding. Keep the values and add a quality flag rather than dropping them silently, so the analyses can decide how to treat them. |
| **Timezone not declared**: timestamps do not follow daylight saving (24 hours on DST days) | Fixed-offset local standard time (UTC+1), confirmed by the offset ARPAE publishes in its REST archive. Converted to UTC in storage. Europe/Rome is used only for presentation. |
