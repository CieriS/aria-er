with limits as (

    select pollutant_id from {{ ref('air_quality_limits') }}
    where metric = 'daily_max_8h_mean'

),

hourly as (

    select measurements.*
    from {{ ref('int_measurements_deduplicated') }} as measurements
    inner join limits on measurements.pollutant_id = limits.pollutant_id
    where measurements.is_valid

),

rolling as (

    select
        station_id,
        pollutant_id,
        pollutant_code,
        measured_at_utc,
        -- A window belongs to the day in which it ends (the 16:00-24:00 one to that day).
        measurement_date,
        avg(concentration_ugm3) over eight_hours as rolling_8h_mean_ugm3,
        count(*) over eight_hours as valid_hours_in_window
    from hourly
    window eight_hours as (
        partition by station_id, pollutant_id
        order by measured_at_utc
        range between interval 7 hour preceding and current row
    )

)

select
    *,
    valid_hours_in_window / 8 >= {{ var('min_coverage') }} as is_valid_window
from rolling
