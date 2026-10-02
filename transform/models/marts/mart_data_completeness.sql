with daily as (

    select * from {{ ref('int_measurements_daily') }}

),

series as (

    select
        station_id,
        pollutant_id,
        pollutant_code,
        expected_measurements,
        min(measurement_date) as first_date,
        max(measurement_date) as last_date
    from daily
    group by all

),

-- Expected grid: every day between the first and last observation of each series.
-- Missing data are absent rows upstream, so gaps only show against this grid.
expected as (

    select
        series.station_id,
        series.pollutant_id,
        series.pollutant_code,
        series.expected_measurements,
        cast(days.generate_series as date) as measurement_date
    from series,
        generate_series(series.first_date, series.last_date, interval 1 day) as days

)

select
    expected.station_id,
    stations.station_name,
    stations.municipality,
    expected.pollutant_id,
    expected.pollutant_code,
    expected.measurement_date,
    expected.expected_measurements,
    coalesce(daily.measurements, 0) as measurements,
    coalesce(daily.valid_measurements, 0) as valid_measurements,
    coalesce(daily.valid_measurements, 0) / expected.expected_measurements as completeness,
    coalesce(daily.is_valid_day, false) as is_complete
from expected
left join daily
    on expected.station_id = daily.station_id
    and expected.pollutant_id = daily.pollutant_id
    and expected.measurement_date = daily.measurement_date
left join {{ ref('int_stations_current') }} as stations
    on expected.station_id = stations.station_id
