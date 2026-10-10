with limits as (

    select * from {{ ref('air_quality_limits') }}

),

daily_means as (

    select
        station_id,
        pollutant_id,
        'daily_mean' as metric,
        measurement_date,
        daily_mean_ugm3 as value_ugm3,
        1 as periods_per_day
    from {{ ref('int_measurements_daily') }}
    where is_valid_day

),

hourly_means as (

    select
        station_id,
        pollutant_id,
        'hourly_mean' as metric,
        measurement_date,
        concentration_ugm3 as value_ugm3,
        24 as periods_per_day
    from {{ ref('int_measurements_deduplicated') }}
    where is_valid and averaging_minutes = 60

),

daily_max_8h_means as (

    select
        station_id,
        pollutant_id,
        'daily_max_8h_mean' as metric,
        measurement_date,
        max(rolling_8h_mean_ugm3) as value_ugm3,
        1 as periods_per_day
    from {{ ref('int_o3_8h_rolling') }}
    where is_valid_window
    group by station_id, pollutant_id, measurement_date
    -- A day counts only with enough valid 8-hour windows.
    having count(*) / 24 >= {{ var('min_coverage') }}

),

periods as (

    select * from daily_means
    union all
    select * from hourly_means
    union all
    select * from daily_max_8h_means

),

yearly as (

    select
        periods.station_id,
        periods.pollutant_id,
        periods.metric,
        extract(year from periods.measurement_date) as year,
        count(case when periods.value_ugm3 > limits.limit_ugm3 then 1 end) as exceedances,
        count(*) as valid_periods,
        max(periods.value_ugm3) as max_value_ugm3,
        any_value(periods.periods_per_day) as periods_per_day
    from periods
    inner join limits
        on periods.pollutant_id = limits.pollutant_id
        and periods.metric = limits.metric
    group by periods.station_id, periods.pollutant_id, periods.metric, year

)

select
    yearly.station_id,
    stations.station_name,
    stations.municipality,
    yearly.pollutant_id,
    limits.pollutant_code,
    yearly.metric,
    yearly.year,
    yearly.exceedances,
    limits.limit_ugm3,
    limits.max_exceedances_per_year,
    yearly.exceedances > limits.max_exceedances_per_year as is_over_allowed_exceedances,
    yearly.max_value_ugm3,
    yearly.valid_periods,
    yearly.valid_periods / (
        yearly.periods_per_day
        * {{ days_in_year('yearly.year') }}
    ) as year_coverage
from yearly
inner join limits
    on yearly.pollutant_id = limits.pollutant_id
    and yearly.metric = limits.metric
left join {{ ref('int_stations_current') }} as stations
    on yearly.station_id = stations.station_id
