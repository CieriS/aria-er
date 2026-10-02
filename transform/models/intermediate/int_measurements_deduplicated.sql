{{
    config(
        materialized='incremental',
        unique_key=['station_id', 'pollutant_id', 'measured_at_utc'],
        incremental_strategy='delete+insert'
    )
}}

with unioned as (

    select
        station_id,
        pollutant_id,
        measured_at_utc,
        concentration_ugm3,
        value_original,
        unit_original,
        validation_flag,
        'near_real_time' as source_name,
        2 as source_priority
    from {{ ref('stg_arpae__measurements') }}

    union all

    select
        station_id,
        pollutant_id,
        measured_at_utc,
        concentration_ugm3,
        value_original,
        unit_original,
        null as validation_flag,
        'historical' as source_name,
        1 as source_priority
    from {{ ref('stg_arpae__measurements_historical') }}

),

in_scope as (

    select * from unioned
    {% if is_incremental() %}
    -- Reprocess the same window the ingestor rewrites, to pick up revised values.
    where measured_at_utc >= (
        select max(measured_at_utc) - interval ({{ var('lookback_days') }}) day from {{ this }}
    )
    {% endif %}

),

deduplicated as (

    -- The validated archive wins over the near-real-time feed for the same key.
    select * from in_scope
    qualify row_number() over (
        partition by station_id, pollutant_id, measured_at_utc
        order by source_priority
    ) = 1

),

enriched as (

    select
        deduplicated.station_id,
        deduplicated.pollutant_id,
        pollutants.pollutant_code,
        pollutants.averaging_minutes,
        deduplicated.measured_at_utc,
        -- Hourly values are stamped at the end of the hour, daily values on the day.
        case
            when pollutants.averaging_minutes = 60
                then deduplicated.measured_at_utc - interval 1 hour
            else deduplicated.measured_at_utc
        end as period_start_utc,
        deduplicated.concentration_ugm3,
        deduplicated.value_original,
        deduplicated.unit_original,
        deduplicated.validation_flag,
        deduplicated.source_name,
        deduplicated.concentration_ugm3 < 0 as is_negative,
        deduplicated.concentration_ugm3 = 0 as is_zero,
        coalesce(deduplicated.concentration_ugm3 > pollutants.max_plausible_ugm3, false)
            as is_implausible
    from deduplicated
    left join {{ ref('arpae_pollutants') }} as pollutants
        on deduplicated.pollutant_id = pollutants.pollutant_id

)

select
    station_id,
    pollutant_id,
    pollutant_code,
    averaging_minutes,
    measured_at_utc,
    period_start_utc,
    -- Calendar day in ARPAE local standard time, the day legal limits refer to.
    cast(
        period_start_utc + interval ({{ var('local_utc_offset_hours') }}) hour as date
    ) as measurement_date,
    concentration_ugm3,
    value_original,
    unit_original,
    validation_flag,
    source_name,
    coalesce(is_negative, false) as is_negative,
    coalesce(is_zero, false) as is_zero,
    is_implausible,
    concentration_ugm3 is not null and not is_negative and not is_implausible as is_valid
from enriched
