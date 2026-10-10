-- The traffic vs background mart silently empties if no station is classified as
-- traffic or as background (it happened when ARPAE changed the bulletin labels).
-- One row is returned for each exposure with no station.
select expected.station_type
from (select 'traffic' as station_type union all select 'background') as expected
left join {{ ref('int_station_types_current') }} as types
    on expected.station_type = types.station_type
group by expected.station_type
having count(types.station_id) = 0
