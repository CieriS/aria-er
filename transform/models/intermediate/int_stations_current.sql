with registry as (

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

),

-- Stations of the local networks are measured but missing from the registry: the type
-- registry still gives their name and province.
type_registry as (

    select
        station_id,
        bulletin_station_name as station_name,
        province
    from {{ ref('stg_arpae__station_types') }}
    where true
    qualify row_number() over (partition by station_id order by extracted_on desc) = 1

),

unioned as (

    select * from registry

    union all

    select
        type_registry.station_id,
        type_registry.station_name,
        null as municipality,
        type_registry.province,
        null as address,
        null as altitude_m,
        null as longitude,
        null as latitude
    from type_registry
    left join registry
        on type_registry.station_id = registry.station_id
    where registry.station_id is null

)

select
    *,
    {{ weather_location_id('latitude', 'longitude') }} as weather_location_id
from unioned
