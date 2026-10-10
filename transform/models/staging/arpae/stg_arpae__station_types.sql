with source as (

    select * from {{ source('arpae_raw', 'station_types') }}

)

select
    cast(station_id as integer) as station_id,
    station_name as bulletin_station_name,
    province,
    -- Until 2026-10-05 labels read like 'Urbana Traffico' (an area and an exposure);
    -- since then the bulletin publishes the area only.
    case
        when lower(type_label) like '%traffico%' then 'traffic'
        when lower(type_label) like '%fondo%' then 'background'
        when lower(type_label) like '%industriale%' then 'industrial'
    end as station_type,
    case
        when lower(type_label) like '%remot%' then 'remote'
        when lower(type_label) like '%suburban%' then 'suburban'
        when lower(type_label) like '%urban%' then 'urban'
        when lower(type_label) like '%rural%' then 'rural'
    end as area_type,
    type_label as type_label_original,
    bulletin_id,
    extracted_on
from source
