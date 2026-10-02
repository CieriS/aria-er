with source as (

    select * from {{ source('arpae_raw', 'stations') }}

),

latest as (

    select * from source
    where extracted_on = (select max(extracted_on) from source)

)

select
    cast(station_id as integer) as station_id,
    cast(pollutant_id as integer) as pollutant_id,
    station_name,
    municipality,
    province,
    address,
    altitude_m,
    longitude,
    latitude,
    pollutant_name,
    unit as unit_original,
    extracted_on
from latest
