with hourly as (

    select
        *,
        -- Same convention as the air quality data: a value stamped at the end of its
        -- hour belongs to the local standard day in which that hour starts.
        cast(
            observed_at_utc
            - interval 1 hour
            + interval ({{ var('local_utc_offset_hours') }}) hour
            as date
        ) as weather_date
    from {{ ref('stg_openmeteo__weather') }}

),

daily as (

    select
        location_id,
        weather_date,
        avg(wind_speed_ms) as wind_speed_mean_ms,
        max(wind_speed_ms) as wind_speed_max_ms,
        sum(precipitation_mm) as precipitation_mm,
        avg(temperature_c) as temperature_mean_c,
        avg(surface_pressure_hpa) as surface_pressure_mean_hpa,
        count(*) filter (
            where wind_speed_ms is not null and precipitation_mm is not null
        ) as hours
    from hourly
    group by all

)

select
    *,
    hours / 24 >= {{ var('min_coverage') }} as is_valid_day
from daily
