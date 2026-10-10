# Weather (Open-Meteo)

The second source and what it shows about PM10.

## Weather (Open-Meteo)

Hourly temperature, precipitation, wind speed and direction and surface pressure come from
the [Open-Meteo archive API](https://open-meteo.com/en/docs/historical-weather-api)
(`archive-api.open-meteo.com/v1/archive`), a reanalysis at roughly 10 km, queried in UTC.

- **Where.** At the coordinates of the ARPAE stations, rounded to one decimal (~11 km):
  the 54 stations of the registry collapse to 41 weather locations. A station is matched
  to its location by the same rounding in dbt (`weather_location_id`).
- **Storage.** Same partitioning and upsert as the measurements; natural key
  `(location_id, observed_at)`. Re-running a window leaves the files byte-identical.
- **Daily aggregation.** Days follow ARPAE local standard time with the same hour-ending
  convention as the pollutants, so a PM10 day and its weather day cover the same hours.

### Observed result: wind and PM10 in Bologna

`mart_weather_correlation`, three Bologna stations, 2025 plus August 2026:

| Station | Season | Days | Correlation PM10–wind | Mean PM10, windy days | Mean PM10, other days |
|---|---|---|---|---|---|
| Porta San Felice | winter | 90 | −0.47 | 11.0 µg/m³ (4 days) | 38.0 µg/m³ |
| Giardini Margherita | winter | 81 | −0.42 | 14.2 µg/m³ (5 days) | 34.0 µg/m³ |
| Via Chiarini | winter | 89 | −0.47 | 6.8 µg/m³ (4 days) | 29.3 µg/m³ |
| Porta San Felice | summer | 120 | −0.21 | 15.5 µg/m³ (12 days) | 20.4 µg/m³ |

A windy day is a day with mean wind of at least 3 m/s.

- **Wind**: the correlation is negative in every season at all three stations (−0.17 to
  −0.47) and strongest in winter, when calm, stable air lets particulate accumulate: on
  the few windy winter days PM10 is a third or less of the other days.
- **Rain**: the relation is much weaker (correlation between −0.28 and +0.07). Daily
  rainfall alone says little; one winter series even has slightly higher PM10 on rainy days.
- **Across the region** (42 stations with 2025 data): the winter PM10–wind correlation
  is negative at 41 of them, −0.39 on average, and mean PM10 is 14 µg/m³ on windy winter
  days against 33 on the others. In summer the link weakens (−0.11 on average, negative
  at 33 stations).
- Correlations are plain Pearson coefficients on daily values and say nothing about causes.
