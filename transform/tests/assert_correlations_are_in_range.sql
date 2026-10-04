-- A Pearson correlation lies in [-1, 1].
select station_id, season
from {{ ref('mart_weather_correlation') }}
where abs(corr_pm10_wind) > 1.0000001
    or abs(corr_pm10_precipitation) > 1.0000001
