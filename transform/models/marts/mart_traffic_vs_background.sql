with daily as (

    select
        stations.municipality,
        classification.station_type,
        daily.pollutant_id,
        daily.pollutant_code,
        daily.measurement_date,
        avg(daily.daily_mean_ugm3) as mean_ugm3,
        count(*) as stations
    from {{ ref('int_measurements_daily') }} as daily
    inner join {{ ref('int_station_types_current') }} as classification
        on daily.station_id = classification.station_id
    inner join {{ ref('int_stations_current') }} as stations
        on daily.station_id = stations.station_id
    where daily.is_valid_day
    group by all

),

-- Only days on which both kinds of station have a valid mean, so the two
-- averages cover exactly the same days.
paired as (

    select
        traffic.municipality,
        traffic.pollutant_id,
        traffic.pollutant_code,
        traffic.measurement_date,
        traffic.mean_ugm3 as traffic_ugm3,
        background.mean_ugm3 as background_ugm3,
        traffic.stations as traffic_stations,
        background.stations as background_stations
    from daily as traffic
    inner join daily as background
        on traffic.municipality = background.municipality
        and traffic.pollutant_id = background.pollutant_id
        and traffic.measurement_date = background.measurement_date
    where traffic.station_type = 'traffic'
        and background.station_type = 'background'

)

select
    municipality,
    pollutant_id,
    pollutant_code,
    cast(date_trunc('month', measurement_date) as date) as month,
    count(*) as days,
    max(traffic_stations) as traffic_stations,
    max(background_stations) as background_stations,
    avg(traffic_ugm3) as traffic_mean_ugm3,
    avg(background_ugm3) as background_mean_ugm3,
    avg(traffic_ugm3) - avg(background_ugm3) as difference_ugm3,
    avg(traffic_ugm3) / nullif(avg(background_ugm3), 0) as traffic_to_background_ratio
from paired
group by all
