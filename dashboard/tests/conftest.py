from pathlib import Path

import duckdb
import pytest

from aria_er_dashboard.data import Marts

SCHEMA = """
create table mart_exceedances_yearly as select * from (values
    (7000015, 'PORTA SAN FELICE', 'BOLOGNA', 'PM10', 'daily_mean', 2024, 26, 50.0, 35, false, 90.0, 360, 0.98),
    (7000015, 'PORTA SAN FELICE', 'BOLOGNA', 'PM10', 'daily_mean', 2025, 20, 50.0, 35, false, 103.0, 362, 0.99),
    (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'PM10', 'daily_mean', 2025, 10, 50.0, 35, false, 103.0, 346, 0.95),
    (7000041, 'VIA CHIARINI', 'BOLOGNA', 'O3', 'daily_max_8h_mean', 2025, 54, 120.0, 25, true, 202.8, 355, 0.97)
) as t(station_id, station_name, municipality, pollutant_code, metric, year, exceedances,
       limit_ugm3, max_exceedances_per_year, is_over_allowed_exceedances, max_value_ugm3,
       valid_periods, year_coverage);

create table mart_pollutant_trend as select * from (values
    ('BOLOGNA', 'PM10', 2024, 3, 22.5, 20.0, 25.0, 0.85, false),
    ('BOLOGNA', 'PM10', 2025, 3, 20.6, 17.0, 24.0, 0.95, true),
    ('BOLOGNA', 'NO2', 2025, 3, 20.5, 14.0, 31.0, 0.97, true),
    ('PARMA', 'PM10', 2025, 1, 25.0, 25.0, 25.0, 0.99, true)
) as t(municipality, pollutant_code, year, stations, annual_mean_ugm3, min_station_mean_ugm3,
       max_station_mean_ugm3, min_year_coverage, is_representative);

create table mart_traffic_vs_background as select * from (values
    ('BOLOGNA', 'NO2', date '2025-01-01', 31, 1, 2, 33.0, 23.5, 9.5, 1.40),
    ('BOLOGNA', 'NO2', date '2025-02-01', 28, 1, 2, 40.0, 24.0, 16.0, 1.67),
    ('BOLOGNA', 'PM10', date '2025-01-01', 30, 1, 2, 35.0, 30.5, 4.5, 1.15)
) as t(municipality, pollutant_code, month, days, traffic_stations, background_stations,
       traffic_mean_ugm3, background_mean_ugm3, difference_ugm3, traffic_to_background_ratio);

create table mart_weather_correlation as select * from (values
    (7000015, 'PORTA SAN FELICE', 'BOLOGNA', 'winter', 90, 36.8, -0.47, -0.28, 4, 11.0, 38.0, 25, 34.6, 37.6),
    (7000015, 'PORTA SAN FELICE', 'BOLOGNA', 'summer', 120, 19.9, -0.21, -0.19, 12, 15.5, 20.4, 21, 16.3, 20.7),
    (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'winter', 81, 32.8, -0.42, -0.14, 5, 14.2, 34.0, 21, 35.1, 32.0),
    (2000003, 'CITTADELLA', 'PARMA', 'summer', 12, 20.0, null, null, 1, 15.0, 20.5, 2, 18.0, 20.4)
) as t(station_id, station_name, municipality, season, days, pm10_mean_ugm3, corr_pm10_wind,
       corr_pm10_precipitation, windy_days, pm10_mean_windy_ugm3, pm10_mean_calm_ugm3,
       rainy_days, pm10_mean_rainy_ugm3, pm10_mean_dry_ugm3);

create table mart_data_completeness as select * from (values
    (7000014, 'GIARDINI MARGHERITA', 'NO2', date '2025-10-29', 24, 24, 1.0, true),
    (7000014, 'GIARDINI MARGHERITA', 'NO2', date '2025-10-30', 24, 2, 0.083, false),
    (7000014, 'GIARDINI MARGHERITA', 'NO2', date '2025-10-31', 24, 0, 0.0, false),
    (7000014, 'GIARDINI MARGHERITA', 'PM10', date '2025-10-30', 1, 1, 1.0, true),
    (7000015, 'PORTA SAN FELICE', 'NO2', date '2025-10-30', 24, 24, 1.0, true)
) as t(station_id, station_name, pollutant_code, measurement_date, expected_measurements,
       valid_measurements, completeness, is_complete);
"""


@pytest.fixture
def warehouse_path(tmp_path: Path) -> Path:
    """A DuckDB file with a few synthetic rows in every mart."""
    path = tmp_path / "warehouse.duckdb"
    with duckdb.connect(str(path)) as connection:
        connection.execute(SCHEMA)
    return path


@pytest.fixture
def marts(warehouse_path: Path) -> Marts:
    return Marts(warehouse_path)
