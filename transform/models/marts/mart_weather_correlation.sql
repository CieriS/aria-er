with pm10 as (

    select station_id, measurement_date, daily_mean_ugm3 as pm10_ugm3
    from {{ ref('int_measurements_daily') }}
    where pollutant_code = 'PM10' and is_valid_day

),

stations as (

    select * from {{ ref('int_stations_current') }}
    where weather_location_id is not null

),

weather as (

    select * from {{ ref('int_weather_daily') }}
    where is_valid_day

),

paired as (

    select
        pm10.station_id,
        stations.station_name,
        stations.municipality,
        pm10.measurement_date,
        case
            when extract(month from pm10.measurement_date) in (12, 1, 2) then 'winter'
            when extract(month from pm10.measurement_date) in (3, 4, 5) then 'spring'
            when extract(month from pm10.measurement_date) in (6, 7, 8) then 'summer'
            else 'autumn'
        end as season,
        pm10.pm10_ugm3,
        weather.wind_speed_mean_ms,
        weather.precipitation_mm,
        weather.wind_speed_mean_ms >= {{ var('windy_day_min_wind_ms') }} as is_windy,
        weather.precipitation_mm >= {{ var('rainy_day_min_precipitation_mm') }} as is_rainy
    from pm10
    inner join stations on pm10.station_id = stations.station_id
    inner join weather
        on stations.weather_location_id = weather.location_id
        and pm10.measurement_date = weather.weather_date

),

aggregated as (

    select
        station_id,
        station_name,
        municipality,
        season,
        count(*) as days,
        avg(pm10_ugm3) as pm10_mean_ugm3,
        corr(pm10_ugm3, wind_speed_mean_ms) as corr_pm10_wind,
        corr(pm10_ugm3, precipitation_mm) as corr_pm10_precipitation,
        count(case when is_windy then 1 end) as windy_days,
        avg(case when is_windy then pm10_ugm3 end) as pm10_mean_windy_ugm3,
        avg(case when not is_windy then pm10_ugm3 end) as pm10_mean_calm_ugm3,
        count(case when is_rainy then 1 end) as rainy_days,
        avg(case when is_rainy then pm10_ugm3 end) as pm10_mean_rainy_ugm3,
        avg(case when not is_rainy then pm10_ugm3 end) as pm10_mean_dry_ugm3
    from paired
    group by all

)

select
    station_id,
    station_name,
    municipality,
    season,
    days,
    pm10_mean_ugm3,
    -- A correlation on a handful of days is noise: report it only with enough days.
    case when days >= {{ var('min_days_for_correlation') }} then corr_pm10_wind end
        as corr_pm10_wind,
    case when days >= {{ var('min_days_for_correlation') }} then corr_pm10_precipitation end
        as corr_pm10_precipitation,
    windy_days,
    pm10_mean_windy_ugm3,
    pm10_mean_calm_ugm3,
    rainy_days,
    pm10_mean_rainy_ugm3,
    pm10_mean_dry_ugm3
from aggregated
