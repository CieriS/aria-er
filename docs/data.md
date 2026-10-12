# Data sources and quality

Where the data comes from and how its problems are handled. The full exploration report is in [data-exploration.md](data-exploration.md).

## Data sources

Recent measurements and station types come from the ARPAE REST service
(`https://apps.arpae.it/REST/`); validated history and the station registry from the ARPAE CKAN
dataset [`qualita-dell-aria-rete-di-monitoraggio`](https://dati.arpae.it/dataset/qualita-dell-aria-rete-di-monitoraggio).

| Source | Access | Coverage |
|---|---|---|
| Recent measurements | REST `qa_archivio_dati_public` | About 10.5 million records from late 2021 onwards: 68 stations in 2026, hourly and daily values with offset, unit and validation stages |
| Historical measurements | Google Drive folder, one CSV per year × station × pollutant | 2010–2025, validated |
| Station registry | Google Sheets CSV export | Current stations and their pollutants |
| Station types | REST `qa_stazioni` | 77 stations: exposure (`tipo_stazione`), area (`zona`), coordinates, network |
| Station id map | REST `bollettini_qa` (read once, by hand) | Archive station id (`originalidstazione`) next to the registry code (`idstazione`) |

Until 2026-10-10 recent measurements came from the CKAN datastore (resource
`4dc855a1-6298-4b71-a1ae-d80693d43dcb`), which stopped being updated on 2026-09-17
([ADR 0008](adr/0008-rest-archive-as-the-measurement-source.md)).

Full details, schema and data quality findings: [docs/data-exploration.md](data-exploration.md).

## The REST measurement archive

One record per station, parameter and reference time:

```json
{"_id": "1103_32_2069_202609160000", "station_id": "1103", "station_name": "Porta San Felice (BO)",
 "parameter_id": "32", "parameter_name": "NO2", "series_name": "NO2",
 "reftime": "2026-09-16T00:00:00+01:00", "value": 13.53, "parameter_unit": "µg/m³",
 "validity": "Giornaliera + Mensile", "_updated": "..."}
```

The service is [Eve](https://docs.python-eve.org/): `where` takes a MongoDB-style filter,
`sort` a field, `max_results` and `page` paginate, `_meta.total` counts the matches. The
ingestor asks for the reference times of the window and the parameters it knows, sorted by
`_id`, 2000 records a page (a day of the whole region is about 5900 records). Larger pages
are accepted but the server often drops the connection halfway through them.

What the ingestor changes before writing, and why:

| Published | Stored | Checked against |
|---|---|---|
| Station id `1103` | Registry code `7000015`, from `arpae.station_ids` | The daily bulletin lists both ids for 66 stations; the mobile laboratories of Bologna and Rimini by name |
| Parameter id `32` | Registry id `8`, from `arpae.pollutant_ids` | Values of the CKAN datastore for the same station and hour: equal after rounding for all eleven parameters |
| Hourly value at the **start** of its hour | End of the hour (+1 h) | Same comparison, and `DATA_FINE` of the yearly files of 2025 |
| Daily value at the **end** of its day | The day itself (−24 h) | `DATA_INIZIO` of the yearly files; the daily bulletin of the same day |
| Value, unit, `validity`, `reftime` | Unchanged | — |

Ethylbenzene (`41`) and m,p-xylene (`45`) have no id in the ARPAE parameter registry and are
not requested. A station missing from the map is left out and named in a warning.

## Validation stages

`validity` lists the validations a value has passed: `Non validato` for the first hours,
then `Giornaliera`, `Giornaliera + Mensile`, up to `Giornaliera + Mensile + Semestrale + Annuale`.
A value can be revised at each stage, which is why every run reprocesses a rolling window.
The text is stored as published and no model depends on its meaning. Rows ingested from the
CKAN datastore carried its one-letter flag instead (`G` daily, `M` monthly).

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
  (the PM10 of 23 February 2025 is at `2025-02-24T00:00:00+01:00`), unlike the yearly files.
  Taken together with the bulletin change below, ARPAE appears to be moving to a new platform.
- **2026-10-11, REST archive**: it is now the source of recent measurements. Its last write
  dates from 2026-10-09 13:20 UTC and its newest value from 11:00 of that day (a Friday): on
  the following Sunday nothing had been added. Whether it is fed on working days only is
  not known yet.
- **2026-10-11, daily values of the CKAN datastore**: comparing the datastore with the
  bulletins showed that it dated a daily value at the end of its day too (the PM10 the
  bulletin of 9 August reports is the one the datastore dated 10 August), against the note
  in the parameter registry ("the day reported is the day of measurement") the pipeline
  relied on. PM10 and PM2.5 of 2026 were one day late until the measurements were ingested
  again from the REST archive.

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
| **Data revised after publication** (not validated, then validated daily, monthly, six-monthly, yearly) | Keep the validation stages in raw. Reprocess a rolling window (default 30 days) on every run and upsert by natural key `(station_id, pollutant, period_start)`, so revised values replace provisional ones. Validated history overrides near-real-time data for the same key. |
| **Inconsistent formats**: station code as `7000014`, `07000014` or `7.000.014`; dates as `DD/MM` vs `MM/DD`; CO in mg/m³ | Parse each source with its explicit format and fail loudly on mismatch. Normalise the station code to a single canonical integer in staging. Convert units to µg/m³ in staging, keeping the original value and unit in raw. |
| **Many exact zeros** (e.g. 242 hourly NO values at one station) | Likely below the detection limit or rounding. Keep the values and add a quality flag rather than dropping them silently, so the analyses can decide how to treat them. |
| **Timezone not declared**: timestamps do not follow daylight saving (24 hours on DST days) | Fixed-offset local standard time (UTC+1), confirmed by the offset ARPAE publishes in its REST archive. Converted to UTC in storage. Europe/Rome is used only for presentation. |
