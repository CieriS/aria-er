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

series as (

    select
        station_id,
        pollutant_id,
        pollutant_code,
        min(measurement_date) as first_date,
        max(measurement_date) as last_date
    from hourly
    group by station_id, pollutant_id, pollutant_code

),

hours_of_day as (

    {% for hour in range(24) -%}
    select {{ hour }} as hour_of_day{{ " union all" if not loop.last }}
    {% endfor %}

),

-- Every hour of every day of a series, measured or not: a window that ends on a missing
-- hour still exists, and is valid if enough of its other hours are there.
expected as (

    select
        series.station_id,
        series.pollutant_id,
        series.pollutant_code,
        cast(spine_date as date) as measurement_date,
        -- End of the hour, in UTC, of the hour starting at `hour_of_day` local standard time.
        {{ dbt.dateadd(
            'hour',
            'hours_of_day.hour_of_day + 1 - ' ~ var('local_utc_offset_hours'),
            'cast(spine_date as ' ~ naive_timestamp_type() ~ ')'
        ) }} as measured_at_utc
    from series
    cross join hours_of_day,
        {{ unnest_days('series.first_date', 'series.last_date') }}

),

rolling as (

    select
        expected.station_id,
        expected.pollutant_id,
        expected.pollutant_code,
        expected.measured_at_utc,
        -- A window belongs to the day in which it ends (the 16:00-24:00 one to that day).
        expected.measurement_date,
        avg(hourly.concentration_ugm3) over eight_hours as rolling_8h_mean_ugm3,
        count(hourly.concentration_ugm3) over eight_hours as valid_hours_in_window
    from expected
    left join hourly
        on expected.station_id = hourly.station_id
        and expected.pollutant_id = hourly.pollutant_id
        and expected.measured_at_utc = hourly.measured_at_utc
    -- The grid has no gaps, so eight rows are eight consecutive hours.
    window eight_hours as (
        partition by expected.station_id, expected.pollutant_id
        order by expected.measured_at_utc
        rows between 7 preceding and current row
    )

)

select
    *,
    valid_hours_in_window / 8 >= {{ var('min_coverage') }} as is_valid_window
from rolling
