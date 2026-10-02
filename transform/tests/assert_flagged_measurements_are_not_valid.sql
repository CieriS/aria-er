-- A flagged measurement must never be marked valid, or it would enter the aggregates.
select station_id, pollutant_id, measured_at_utc
from {{ ref('int_measurements_deduplicated') }}
where is_valid and (is_negative or is_implausible or concentration_ugm3 is null)
