-- A day cannot hold more values than its averaging period allows, and a mean of
-- valid values cannot be negative.
select station_id, pollutant_id, measurement_date
from {{ ref('int_measurements_daily') }}
where measurements > expected_measurements
    or daily_mean_ugm3 < 0
