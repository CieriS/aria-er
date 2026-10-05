with source as (

    select * from {{ source('arpae_raw', 'station_types') }}

),

latest as (

    select * from source
    where extracted_on = (select max(extracted_on) from source)

)

select
    cast(station_id as integer) as station_id,
    station_name as bulletin_station_name,
    province,
    -- Labels read like 'Urbana Traffico': an area word and an exposure word.
    case
        when type_label ilike '%traffico%' then 'traffic'
        when type_label ilike '%fondo%' then 'background'
        when type_label ilike '%industriale%' then 'industrial'
        else 'other'
    end as station_type,
    case
        when type_label ilike '%suburban%' then 'suburban'
        when type_label ilike '%urban%' then 'urban'
        when type_label ilike '%rural%' then 'rural'
        when type_label ilike '%remot%' then 'remote'
    end as area_type,
    type_label as type_label_original,
    bulletin_id,
    extracted_on
from latest
