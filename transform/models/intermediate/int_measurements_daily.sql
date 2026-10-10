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
        avg(case when is_valid then concentration_ugm3 end) as daily_mean_ugm3,
        max(case when is_valid then concentration_ugm3 end) as daily_max_ugm3,
        count(*) as measurements,
        count(case when is_valid then 1 end) as valid_measurements,
        cast(1440 / averaging_minutes as integer) as expected_measurements
    from measurements
    group by station_id, pollutant_id, pollutant_code, averaging_minutes, measurement_date

)

select
    *,
    valid_measurements / expected_measurements as coverage,
    valid_measurements / expected_measurements >= {{ var('min_coverage') }} as is_valid_day
from daily
