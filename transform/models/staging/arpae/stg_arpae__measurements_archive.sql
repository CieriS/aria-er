with source as (

    select * from {{ source('arpae_raw', 'measurements_archive') }}

)

select
    cast(station_id as integer) as station_id,
    cast(pollutant_id as integer) as pollutant_id,
    {{ to_naive_utc('measured_at') }} as measured_at_utc,
    {{ to_ugm3('value', 'unit') }} as concentration_ugm3,
    value as value_original,
    unit as unit_original,
    raw_reftime
from source
