with daily as (

    select * from {{ ref('int_measurements_daily') }}
    where is_valid_day

),

station_years as (

    select
        station_id,
        pollutant_id,
        pollutant_code,
        extract(year from measurement_date) as year,
        avg(daily_mean_ugm3) as annual_mean_ugm3,
        count(*) as valid_days
    from daily
    group by station_id, pollutant_id, pollutant_code, year

),

covered as (

    select
        *,
        valid_days / {{ days_in_year('year') }}
            as year_coverage
    from station_years

)

select
    registry.municipality,
    station_years.pollutant_id,
    station_years.pollutant_code,
    station_years.year,
    count(*) as stations,
    avg(station_years.annual_mean_ugm3) as annual_mean_ugm3,
    min(station_years.annual_mean_ugm3) as min_station_mean_ugm3,
    max(station_years.annual_mean_ugm3) as max_station_mean_ugm3,
    min(station_years.year_coverage) as min_year_coverage,
    min(station_years.year_coverage) >= {{ var('min_annual_coverage') }} as is_representative
from covered as station_years
inner join {{ ref('int_stations_current') }} as registry
    on station_years.station_id = registry.station_id
group by registry.municipality, station_years.pollutant_id, station_years.pollutant_code, station_years.year
