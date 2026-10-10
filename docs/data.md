# Data sources and quality

Where the data comes from and how its problems are handled. The full exploration report is in [data-exploration.md](data-exploration.md).

## Data sources

All sources come from the ARPAE CKAN portal, dataset
[`qualita-dell-aria-rete-di-monitoraggio`](https://dati.arpae.it/dataset/qualita-dell-aria-rete-di-monitoraggio).

| Source | Access | Coverage |
|---|---|---|
| Near-real-time measurements | CKAN datastore, resource `4dc855a1-6298-4b71-a1ae-d80693d43dcb`, via `datastore_search_sql` | Last ~7 weeks, with a provisional/validated flag |
| Historical measurements | Google Drive folder, one CSV per year × station × pollutant | 2010–2025, validated |
| Station registry | Google Sheets CSV export | Current stations and their pollutants |
| Pollutant registry | Google Sheets CSV export | IDs, units, averaging period |

Full details, schema and data quality findings: [docs/data-exploration.md](data-exploration.md).

## Upstream changes observed

- **2026-10-06, daily bulletin**: `tipostazione` went from `Urbana Traffico` / `Urbana Fondo` to
  the area alone (`Urbana`), and the records gained `locality` and `originalidstazione`. Nothing
  failed: the ingestor stores the label as published, and the classification downstream silently
  became "unknown" for every station. See [limitations.md](limitations.md) for how it is handled.

## Data quality findings and how they are handled

Phase 0 analysed one year (2025) of data for the Bologna stations. Each issue maps to a design decision:

| Issue found | Handling |
|---|---|
| **Missing hours are absent rows**, not nulls (coverage 94–99.5%, gaps up to 4+ days) | Never fill in raw. Build an expected time grid (station × pollutant × hour/day) and compare it with the actual data to compute completeness in `mart_data_completeness`. Aggregates (e.g. daily means) are only valid above a minimum coverage threshold (75% by EU convention). |
| **Provisional vs validated data** (`v_flag` `M`/`G` in the near-real-time feed, meaning undocumented) | Keep the flag in raw. Reprocess a rolling window (default 30 days) on every run and upsert by natural key `(station_id, pollutant, period_start)`, so revised values replace provisional ones. Validated history overrides near-real-time data for the same key. |
| **Inconsistent formats**: station code as `7000014`, `07000014` or `7.000.014`; dates as `DD/MM` vs `MM/DD`; CO in mg/m³ | Parse each source with its explicit format and fail loudly on mismatch. Normalise the station code to a single canonical integer in staging. Convert units to µg/m³ in staging, keeping the original value and unit in raw. |
| **Many exact zeros** (e.g. 242 hourly NO values at one station) | Likely below the detection limit or rounding. Keep the values and add a quality flag rather than dropping them silently, so the analyses can decide how to treat them. |
| **Timezone not declared**: timestamps do not follow daylight saving (24 hours on DST days) | Treated as fixed-offset local standard time (UTC+1) pending confirmation from ARPAE. Converted to UTC in storage. Europe/Rome is used only for presentation. |
