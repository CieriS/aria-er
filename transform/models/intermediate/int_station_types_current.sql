with snapshots as (

    select * from {{ ref('stg_arpae__station_types') }}

),

latest as (

    select * from snapshots
    where true
    qualify row_number() over (partition by station_id order by extracted_on desc) = 1

),

-- The most recent snapshot stating whether the station measures traffic or background.
-- Snapshots taken from the daily bulletin between 2026-10-06 and 2026-10-08 do not.
latest_with_exposure as (

    select * from snapshots
    where station_type is not null
    qualify row_number() over (partition by station_id order by extracted_on desc) = 1

),

reference as (

    select * from {{ ref('station_types_reference') }}
    where station_type <> 'other'

)

select
    latest.station_id,
    coalesce(latest_with_exposure.station_type, reference.station_type, 'other') as station_type,
    coalesce(latest.area_type, latest_with_exposure.area_type, reference.area_type) as area_type,
    case
        when latest.station_type is not null then 'latest_snapshot'
        when latest_with_exposure.station_type is not null then 'earlier_snapshot'
        when reference.station_type is not null then 'reference'
        else 'unknown'
    end as station_type_source,
    latest.type_label_original,
    latest.bulletin_id,
    latest.extracted_on
from latest
left join latest_with_exposure
    on latest.station_id = latest_with_exposure.station_id
left join reference
    on latest.station_id = reference.station_id
