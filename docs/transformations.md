# Transformations (dbt)

Models, rules, and the checks of the marts against ARPAE reports.

## Transformations (dbt on DuckDB)

`make transform` builds the warehouse in `warehouse/aria_er.duckdb` from the Parquet files
in `raw/` and the archive samples in `data/samples/`. It needs `make ingest` to have run at
least once.

| Layer | Model | Content |
|---|---|---|
| staging | `stg_arpae__measurements` | Near-real-time rows: typed, UTC timestamp, value in µg/m³ |
| staging | `stg_arpae__measurements_archive` | Validated archive measurements, same shape as the near-real-time ones |
| staging | `stg_arpae__stations` | Latest extraction of the station registry |
| staging | `stg_arpae__station_types` | Station type snapshots from the ARPAE daily bulletin, label normalised |
| intermediate | `int_station_types_current` | Current exposure and area of each station, and where the exposure comes from |
| snapshot | `snap_arpae__stations` | Type 2 history of the registry |
| intermediate | `int_measurements_deduplicated` | Incremental fact, one row per natural key, with quality flags |
| intermediate | `int_measurements_daily` | Daily mean, maximum and coverage |
| intermediate | `int_o3_8h_rolling` | Rolling 8-hour ozone mean |
| intermediate | `int_stations_current` | Current attributes of each station |
| mart | `mart_exceedances_yearly` | Exceedances of each legal limit per station and year |
| mart | `mart_data_completeness` | Coverage per station, pollutant and day, including empty days |
| staging | `stg_openmeteo__weather` | Hourly weather per location, UTC timestamp |
| intermediate | `int_weather_daily` | Daily wind, precipitation, temperature and pressure per location |
| mart | `mart_weather_correlation` | PM10 vs wind and rain, per station and season |
| mart | `mart_pollutant_trend` | Annual mean per municipality, pollutant and year |
| mart | `mart_traffic_vs_background` | Monthly traffic vs background means in the same municipality |

Seeds: `air_quality_limits` (legal thresholds) and `arpae_pollutants` (averaging period and
plausibility bound per pollutant) and `station_types_reference` (the last station classification
ARPAE published with the exposure, used when the ingested bulletins do not state it).

Rules applied:

- **Legal day.** Days and years are counted in ARPAE local standard time (UTC+1 all year).
  An hourly value stamped at the end of its hour belongs to the day the hour starts in.
- **Minimum coverage 75%.** A daily mean needs 18 of 24 hours, an 8-hour mean 6 of 8 hours,
  a daily 8-hour maximum 18 of 24 windows. Below that the period is not counted.
- **Exceedance** means strictly greater than the limit.
- **Flag, don't drop.** Negative, zero and implausible values stay in the fact table with
  `is_negative`, `is_zero`, `is_implausible`; only `is_valid` rows enter the aggregates.
  Two tests with `warn` severity surface them without failing the build.
- **Incremental lookback of 30 days**, the same window the ingestor reprocesses. Archive rows not
  loaded yet are added whatever their age.
- **Freshness**: `dbt source freshness` warns when the newest measurement is older than 48 hours.

### Check against the ARPAE annual report (Bologna, 2025)

`mart_exceedances_yearly` compared with ARPAE's
[report on the 2025 data of the Bologna network](https://www.arpae.it/it/il-territorio/bologna/report-a-bo/aria/report-annuali-aria-bo):

| Indicator (2025) | Station | aria-er | ARPAE report |
|---|---|---|---|
| PM10 days above 50 µg/m³ | Porta San Felice | 20 | 20 |
| PM10 days above 50 µg/m³ | Giardini Margherita | 10 | 10 |
| PM10 days above 50 µg/m³ | Via Chiarini | 7 | 7 |
| O3 days with 8-hour maximum above 120 µg/m³ | Giardini Margherita | 27 | 27 |
| O3 days with 8-hour maximum above 120 µg/m³ | Via Chiarini | 54 | 54 |
| NO2 hours above 200 µg/m³ | all three | 0 | 0 |

The same data also reproduce figures the marts do not expose yet: NO2 annual means
(31 / 14 / 16 µg/m³), PM10 annual means (24 / 21 / 17 µg/m³) and ozone hours above
180 µg/m³ (2 at Giardini Margherita, 22 at Via Chiarini).

Notes on the comparison:

- PM10 figures are those of the report's ten-year table. Its monthly table for 2025 lists
  7 for Giardini Margherita and 10 for Chiarini, the opposite of its own ten-year table;
  the data agree with the ten-year table.
- The report states that its times are in standard time, which supports the UTC+1 assumption.
- For ozone the law caps the *three-year average* of exceedance days at 25. The mart
  compares each single year with 25, so `is_over_allowed_exceedances` is an approximation
  for ozone until three full years are loaded.
- 2026 rows cover August only (`year_coverage` ≈ 0.08): their counts are partial.

### Ten years of PM10 in Bologna

With the 2016–2024 archive files for PM10, PM2.5, NO2 and O3 of the three Bologna stations,
the marts reproduce the ten-year tables of the same ARPAE report:

- **Annual means** (`mart_pollutant_trend`, highest station): 26, 29, 26, 26, 26, 26, 27, 22,
  25, 24 µg/m³ for 2016–2025, identical to the report's Porta San Felice row.
- **Exceedance days** (`mart_exceedances_yearly`): Porta San Felice matches every year
  (33, 40, 18, 32, 42, 29, 33, 4, 26, 20), and so do Giardini Margherita and Via Chiarini.
  For 2023 the ten-year table of the 2025 report lists 6 and 3 for these two stations,
  while the marts give 3 and 5: the 2023 and 2024 editions of the same report both say 3
  and 5, month by month as in the marts, so the 2025 table has those two cells wrong.
- **NO2 annual means** (`mart_pollutant_trend`, highest station): 52, 46, 49, 46, 38, 44, 39,
  43, 28, 31 µg/m³, the report's Porta San Felice row except 2021 (44 against 43).
- **Traffic vs background** (`mart_traffic_vs_background`, Bologna, 2025): NO2 is on average
  31 µg/m³ at the traffic station against 15 at the background ones, PM10 24 against 19,
  while PM2.5 shows no difference (13.3 against 13.5). Across the nine municipalities with
  both kinds of station, the 2025 NO2 surplus at traffic stations ranges from 5.5 µg/m³
  (Ravenna) to 15.8 (Bologna); for PM10 the regional averages are 25.4 against 22.4.
- **Regional check**: for 2025 the mart finds a single station above the 35 allowed PM10
  exceedance days, Modena – Giardini with 40, as stated in ARPAE's regional summary for 2025.

### Legal limits to verify

The thresholds in `transform/seeds/air_quality_limits.csv` are consistent with the
values quoted in the ARPAE report, but have not been checked line by line against the text of
D.Lgs. 155/2010 in force:

| Pollutant | Metric | Limit | Allowed per year |
|---|---|---|---|
| PM10 | daily mean | 50 µg/m³ | 35 days |
| NO2 | hourly mean | 200 µg/m³ | 18 hours |
| O3 | daily maximum of the 8-hour rolling mean | 120 µg/m³ | 25 days, as a 3-year average (target value, not a limit value) |

Annual-mean limits (PM10 and NO2 40 µg/m³, PM2.5 25 µg/m³) are not in the seed because no
mart uses them yet. Directive (EU) 2024/2881 sets stricter values from 2030.

## Planned marts

| Mart | Question |
|---|---|
| `mart_exceedances_yearly` | How many times each station exceeded legal limits, per year |
| `mart_pollutant_trend` | Multi-year trend by pollutant and city |
| `mart_traffic_vs_background` | Traffic vs background stations in the same area |
| `mart_weather_correlation` | Wind/rain vs PM10 peaks |
| `mart_data_completeness` | Data coverage per station (quality audit) |

Legal limits (Italian D.Lgs. 155/2010) will live in a dbt seed, never hardcoded.
