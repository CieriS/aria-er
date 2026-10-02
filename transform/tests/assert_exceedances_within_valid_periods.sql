-- Exceedances are counted on valid periods only, and a year cannot be more than complete.
select station_id, pollutant_id, metric, year
from {{ ref('mart_exceedances_yearly') }}
where exceedances > valid_periods
    or year_coverage > 1
