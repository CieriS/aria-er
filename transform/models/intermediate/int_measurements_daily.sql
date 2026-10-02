with measurements as (

    select * from {{ ref('int_measurements_deduplicated') }}

),

daily as (

    select
        station_id,
        pollutant_id,
        pollutant_code,
        averaging_minutes,
        measurement_date,
        avg(concentration_ugm3) filter (where is_valid) as daily_mean_ugm3,
        max(concentration_ugm3) filter (where is_valid) as daily_max_ugm3,
        count(*) as measurements,
        count(*) filter (where is_valid) as valid_measurements,
        cast(1440 / averaging_minutes as integer) as expected_measurements
    from measurements
    group by all

)

select
    *,
    valid_measurements / expected_measurements as coverage,
    valid_measurements / expected_measurements >= {{ var('min_coverage') }} as is_valid_day
from daily
