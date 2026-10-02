-- Negative concentrations are kept and flagged (is_negative), never dropped.
-- This test only surfaces them.
{{ config(severity='warn') }}

select station_id, pollutant_id, measured_at_utc, concentration_ugm3
from {{ ref('int_measurements_deduplicated') }}
where is_negative
