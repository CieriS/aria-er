with source as (

    select * from {{ source('openmeteo_raw', 'weather') }}

)

select
    location_id,
    latitude,
    longitude,
    grid_latitude,
    grid_longitude,
    {{ to_naive_utc('observed_at') }} as observed_at_utc,
    temperature_c,
    precipitation_mm,
    wind_speed_ms,
    wind_direction_deg,
    surface_pressure_hpa
from source
