-- The latest bulletin no longer states the exposure of these stations: their type comes
-- from an earlier bulletin or from the reference seed and may be out of date.
{{ config(severity='warn') }}

select station_id, station_type, station_type_source, type_label_original
from {{ ref('int_station_types_current') }}
where station_type_source in ('earlier_bulletin', 'reference')
