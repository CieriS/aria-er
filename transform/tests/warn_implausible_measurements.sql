-- Values above the plausibility bound of the pollutant are kept and flagged
-- (is_implausible), never dropped. This test only surfaces them.
{{ config(severity='warn') }}

select station_id, pollutant_id, measured_at_utc, concentration_ugm3
from {{ ref('int_measurements_deduplicated') }}
where is_implausible
