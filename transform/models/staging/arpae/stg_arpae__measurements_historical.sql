with source as (

    select * from {{ source('arpae_historical', 'measurements') }}

),

parsed as (

    select
        cast(COD_STAZ as integer) as station_id,
        cast(ID_PARAM as integer) as pollutant_id,
        strptime(DATA_INIZIO, '%d/%m/%Y %H') as period_start_local,
        strptime(DATA_FINE, '%d/%m/%Y %H') as period_end_local,
        cast(VALORE as double) as value_original,
        UM as unit_original
    from source

)

select
    station_id,
    pollutant_id,
    -- Same reference time as the near-real-time feed: end of the interval for
    -- hourly values, the day itself for daily values.
    case
        when date_diff('hour', period_start_local, period_end_local) >= 24 then period_start_local
        else period_end_local
    end - interval ({{ var('local_utc_offset_hours') }}) hour as measured_at_utc,
    {{ to_ugm3('value_original', 'unit_original') }} as concentration_ugm3,
    value_original,
    unit_original
from parsed
