import json
import math
from datetime import UTC, date, datetime
from decimal import Decimal
from pathlib import Path

import duckdb
import pytest

from aria_er_orchestration.export import (
    FILES,
    MAX_FILE_BYTES,
    SCHEMA_VERSION,
    ExportTooLargeError,
    export_marts,
    to_json_value,
)

NOW = datetime(2026, 10, 21, 6, 30, tzinfo=UTC)


def make_warehouse(path: Path, *, completeness_rows: bool = True) -> None:
    """A warehouse with the five marts, reduced to the columns the export reads."""
    with duckdb.connect(str(path)) as connection:
        connection.execute(
            """
            create table mart_exceedances_yearly as select * from (values
                (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'PM10', 'daily_mean', 2025, 10,
                 50.0, 35, false, 81.234567, 360, 0.986)
            ) t(station_id, station_name, municipality, pollutant_code, metric, year,
                exceedances, limit_ugm3, max_exceedances_per_year, is_over_allowed_exceedances,
                max_value_ugm3, valid_periods, year_coverage)
            """
        )
        connection.execute(
            """
            create table mart_pollutant_trend as select * from (values
                ('BOLOGNA', 'PM10', 2025, 3, 21.4, 17.0, 24.0, 0.95, true)
            ) t(municipality, pollutant_code, year, stations, annual_mean_ugm3,
                min_station_mean_ugm3, max_station_mean_ugm3, min_year_coverage,
                is_representative)
            """
        )
        connection.execute(
            """
            create table mart_traffic_vs_background as select * from (values
                ('BOLOGNA', 'NO2', date '2025-01-01', 31, 1, 2, 45.0, 30.0, 15.0, 1.5)
            ) t(municipality, pollutant_code, month, days, traffic_stations,
                background_stations, traffic_mean_ugm3, background_mean_ugm3, difference_ugm3,
                traffic_to_background_ratio)
            """
        )
        connection.execute(
            """
            create table mart_weather_correlation as select * from (values
                (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'winter', 90, 40.0, -0.5, 'NaN'::double,
                 10, 30.0, 45.0, 20, 35.0, 42.0)
            ) t(station_id, station_name, municipality, season, days, pm10_mean_ugm3,
                corr_pm10_wind, corr_pm10_precipitation, windy_days, pm10_mean_windy_ugm3,
                pm10_mean_calm_ugm3, rainy_days, pm10_mean_rainy_ugm3, pm10_mean_dry_ugm3)
            """
        )
        connection.execute(
            """
            create table mart_data_completeness (
                station_id integer, station_name varchar, municipality varchar,
                pollutant_code varchar, measurement_date date, completeness double,
                is_complete boolean
            )
            """
        )
        if completeness_rows:
            connection.execute(
                """
                insert into mart_data_completeness values
                    (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'PM10',
                     date '2026-01-30', 1.0, true),
                    (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'PM10',
                     date '2026-01-31', 0.0, false),
                    (7000014, 'GIARDINI MARGHERITA', 'BOLOGNA', 'PM10',
                     date '2026-02-01', 0.5, false)
                """
            )


def read(out: Path, name: str) -> object:
    return json.loads((out / name).read_text(encoding="utf-8"))


def test_json_values_are_plain_and_rounded() -> None:
    assert to_json_value(date(2026, 1, 31)) == "2026-01-31"
    assert to_json_value(datetime(2026, 1, 31, 23, 0, tzinfo=UTC)) == "2026-01-31T23:00:00+00:00"
    assert to_json_value(81.234567) == 81.235
    assert to_json_value(Decimal("1.23456")) == 1.235
    assert to_json_value(math.nan) is None
    assert to_json_value(math.inf) is None
    assert to_json_value(True) is True
    assert to_json_value(7) == 7
    assert to_json_value(None) is None
    assert to_json_value("BOLOGNA") == "BOLOGNA"


def test_every_mart_becomes_a_file_with_its_rows(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse)
    out = tmp_path / "out"

    counts = export_marts(warehouse, out, NOW)

    assert set(counts) == set(FILES)
    exceedances = read(out, "exceedances.json")
    assert exceedances == [
        {
            "station_id": 7000014,
            "station_name": "GIARDINI MARGHERITA",
            "municipality": "BOLOGNA",
            "pollutant_code": "PM10",
            "metric": "daily_mean",
            "year": 2025,
            "exceedances": 10,
            "limit_ugm3": 50.0,
            "max_exceedances_per_year": 35,
            "is_over_allowed_exceedances": False,
            "max_value_ugm3": 81.235,
            "valid_periods": 360,
            "year_coverage": 0.986,
        }
    ]
    traffic = read(out, "traffic-vs-background.json")
    assert isinstance(traffic, list)
    assert traffic[0]["month"] == "2025-01-01"
    weather = read(out, "weather-correlation.json")
    assert isinstance(weather, list)
    assert weather[0]["corr_pm10_precipitation"] is None


def test_completeness_goes_out_per_station_pollutant_and_month(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse)
    out = tmp_path / "out"

    counts = export_marts(warehouse, out, NOW)

    assert counts["completeness-monthly.json"] == 2
    assert read(out, "completeness-monthly.json") == [
        {
            "station_id": 7000014,
            "station_name": "GIARDINI MARGHERITA",
            "municipality": "BOLOGNA",
            "pollutant_code": "PM10",
            "month": "2026-01-01",
            "days": 2,
            "complete_days": 1,
            "completeness": 0.5,
        },
        {
            "station_id": 7000014,
            "station_name": "GIARDINI MARGHERITA",
            "municipality": "BOLOGNA",
            "pollutant_code": "PM10",
            "month": "2026-02-01",
            "days": 1,
            "complete_days": 0,
            "completeness": 0.5,
        },
    ]


def test_meta_states_version_time_period_files_and_credits(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse)
    out = tmp_path / "out"

    counts = export_marts(warehouse, out, NOW)

    meta = read(out, "meta.json")
    assert isinstance(meta, dict)
    assert meta["schema_version"] == SCHEMA_VERSION
    assert meta["generated_at"] == "2026-10-21T06:30:00+00:00"
    assert meta["period"] == {"from": "2026-01-30", "to": "2026-02-01"}
    assert meta["files"] == counts
    assert {credit["source"] for credit in meta["credits"]} == {
        "ARPAE Emilia-Romagna",
        "Open-Meteo",
    }


def test_an_empty_mart_gives_an_empty_list_and_no_period(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse, completeness_rows=False)
    out = tmp_path / "out"

    counts = export_marts(warehouse, out, NOW)

    assert counts["completeness-monthly.json"] == 0
    assert read(out, "completeness-monthly.json") == []
    meta = read(out, "meta.json")
    assert isinstance(meta, dict)
    assert meta["period"] is None


def test_a_file_over_the_size_limit_stops_the_export(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse)
    monkeypatch.setattr("aria_er_orchestration.export.MAX_FILE_BYTES", 100)

    with pytest.raises(ExportTooLargeError, match=r"exceedances\.json"):
        export_marts(warehouse, tmp_path / "out", NOW)


def test_a_missing_mart_names_itself(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    with duckdb.connect(str(warehouse)) as connection:
        connection.execute("create table unrelated (x integer)")

    with pytest.raises(duckdb.CatalogException, match="mart_exceedances_yearly"):
        export_marts(warehouse, tmp_path / "out", NOW)


def test_the_size_limit_is_generous_for_the_real_marts() -> None:
    # The real completeness file is about 1 MB (2016 to 2026); the limit leaves room to grow.
    assert MAX_FILE_BYTES == 2_000_000


# The columns of each file, as docs/export.md documents them: a change here is a contract change.
CONTRACT = {
    "exceedances.json": [
        "station_id", "station_name", "municipality", "pollutant_code", "metric", "year",
        "exceedances", "limit_ugm3", "max_exceedances_per_year", "is_over_allowed_exceedances",
        "max_value_ugm3", "valid_periods", "year_coverage",
    ],
    "pollutant-trend.json": [
        "municipality", "pollutant_code", "year", "stations", "annual_mean_ugm3",
        "min_station_mean_ugm3", "max_station_mean_ugm3", "min_year_coverage",
        "is_representative",
    ],
    "traffic-vs-background.json": [
        "municipality", "pollutant_code", "month", "days", "traffic_stations",
        "background_stations", "traffic_mean_ugm3", "background_mean_ugm3", "difference_ugm3",
        "traffic_to_background_ratio",
    ],
    "weather-correlation.json": [
        "station_id", "station_name", "municipality", "season", "days", "pm10_mean_ugm3",
        "corr_pm10_wind", "corr_pm10_precipitation", "windy_days", "pm10_mean_windy_ugm3",
        "pm10_mean_calm_ugm3", "rainy_days", "pm10_mean_rainy_ugm3", "pm10_mean_dry_ugm3",
    ],
    "completeness-monthly.json": [
        "station_id", "station_name", "municipality", "pollutant_code", "month", "days",
        "complete_days", "completeness",
    ],
}  # fmt: skip


@pytest.mark.parametrize("name", sorted(CONTRACT))
def test_each_file_keeps_the_documented_columns(tmp_path: Path, name: str) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    make_warehouse(warehouse)
    out = tmp_path / "out"

    export_marts(warehouse, out, NOW)

    rows = read(out, name)
    assert isinstance(rows, list)
    assert rows, name
    assert list(rows[0]) == CONTRACT[name]


def test_the_contract_covers_every_file() -> None:
    assert set(CONTRACT) == set(FILES)
