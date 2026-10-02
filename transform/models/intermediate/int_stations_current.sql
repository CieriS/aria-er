select distinct
    station_id,
    station_name,
    municipality,
    province,
    address,
    altitude_m,
    longitude,
    latitude
from {{ ref('snap_arpae__stations') }}
where dbt_valid_to is null
