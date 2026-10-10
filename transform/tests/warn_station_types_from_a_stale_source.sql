-- The latest snapshot does not state the exposure of these stations although an older
-- one or the reference seed does: their type may be out of date. Stations that ARPAE
-- does not classify at all (mobile laboratories) are not listed.
{{ config(severity='warn') }}

select station_id, station_type, station_type_source, type_label_original
from {{ ref('int_station_types_current') }}
where station_type_source in ('earlier_snapshot', 'reference')
