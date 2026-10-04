select distinct
    station_id,
    station_name,
    municipality,
    province,
    address,
    altitude_m,
    longitude,
    latitude,
    {{ weather_location_id('latitude', 'longitude') }} as weather_location_id
from {{ ref('snap_arpae__stations') }}
where dbt_valid_to is null
