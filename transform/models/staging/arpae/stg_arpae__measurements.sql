with source as (

    select * from {{ source('arpae_raw', 'measurements') }}

)

select
    cast(station_id as integer) as station_id,
    cast(pollutant_id as integer) as pollutant_id,
    cast(measured_at at time zone 'UTC' as timestamp) as measured_at_utc,
    {{ to_ugm3('value', 'unit') }} as concentration_ugm3,
    value as value_original,
    unit as unit_original,
    validation_flag,
    raw_reftime
from source
