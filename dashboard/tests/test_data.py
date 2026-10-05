import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from datetime import date
from pathlib import Path

import duckdb
import pytest

from aria_er_dashboard.data import (
    DEFAULT_DUCKDB_PATH,
    Marts,
    MartsUnavailableError,
    duckdb_path_from_env,
)


def test_exceedances_filters_by_metric_station_and_years(marts: Marts) -> None:
    frame = marts.exceedances("daily_mean")
    assert len(frame) == 3
    assert set(frame["pollutant_code"]) == {"PM10"}

    one_station = marts.exceedances("daily_mean", stations=["PORTA SAN FELICE"])
    assert list(one_station["year"]) == [2024, 2025]
    assert list(one_station["exceedances"]) == [26, 20]

    one_year = marts.exceedances("daily_mean", years=(2025, 2025))
    assert set(one_year["year"]) == {2025}
    assert len(one_year) == 2


def test_exceedances_carries_the_legal_threshold_from_the_mart(marts: Marts) -> None:
    frame = marts.exceedances("daily_max_8h_mean")
    assert frame["max_exceedances_per_year"].iloc[0] == 25
    assert frame["limit_ugm3"].iloc[0] == 120.0
    assert bool(frame["is_over_allowed_exceedances"].iloc[0])


def test_an_empty_selection_returns_no_rows_and_none_means_no_filter(marts: Marts) -> None:
    assert marts.exceedances("daily_mean", stations=[]).empty
    assert len(marts.exceedances("daily_mean", stations=None)) == 3
    assert marts.exceedances("unknown_metric").empty


def test_filter_values_are_parameters_not_sql(marts: Marts) -> None:
    hostile = "x' or 1=1 --"
    assert marts.exceedances("daily_mean", stations=[hostile]).empty
    assert marts.exceedances(hostile).empty
    assert marts.completeness(hostile).empty


def test_pollutant_trend_filters(marts: Marts) -> None:
    frame = marts.pollutant_trend("PM10")
    assert list(frame["municipality"]) == ["BOLOGNA", "BOLOGNA", "PARMA"]

    bologna = marts.pollutant_trend("PM10", ["BOLOGNA"], (2025, 2030))
    assert len(bologna) == 1
    assert bologna["annual_mean_ugm3"].iloc[0] == 20.6
    assert bool(bologna["is_representative"].iloc[0])


def test_traffic_vs_background_filters_by_period(marts: Marts) -> None:
    frame = marts.traffic_vs_background("NO2", "BOLOGNA")
    assert list(frame["difference_ugm3"]) == [9.5, 16.0]

    february = marts.traffic_vs_background("NO2", "BOLOGNA", (date(2025, 2, 1), date(2025, 2, 28)))
    assert len(february) == 1
    assert marts.traffic_vs_background("NO2", "PARMA").empty


def test_weather_correlation_filters_and_keeps_missing_correlations(marts: Marts) -> None:
    assert len(marts.weather_correlation()) == 4

    winter = marts.weather_correlation(seasons=["winter"])
    assert set(winter["station_name"]) == {"PORTA SAN FELICE", "GIARDINI MARGHERITA"}

    parma = marts.weather_correlation(stations=["CITTADELLA"])
    assert parma["corr_pm10_wind"].isna().all()


def test_completeness_is_per_station_with_empty_days(marts: Marts) -> None:
    frame = marts.completeness("GIARDINI MARGHERITA")
    assert len(frame) == 4
    assert frame["completeness"].min() == 0.0

    no2 = marts.completeness(
        "GIARDINI MARGHERITA", ["NO2"], (date(2025, 10, 30), date(2025, 10, 31))
    )
    assert list(no2["valid_measurements"]) == [2, 0]


def test_distinct_and_date_range_feed_the_filters(marts: Marts) -> None:
    assert marts.distinct("mart_pollutant_trend", "municipality") == ["BOLOGNA", "PARMA"]
    assert marts.distinct("mart_exceedances_yearly", "year") == [2024, 2025]
    # Null correlations are not offered as values.
    assert None not in marts.distinct("mart_weather_correlation", "corr_pm10_wind")
    assert marts.date_range("mart_data_completeness", "measurement_date") == (
        date(2025, 10, 29),
        date(2025, 10, 31),
    )


def test_only_mart_columns_can_be_listed(marts: Marts) -> None:
    for mart, column in [
        ("stg_arpae__measurements", "station_id"),
        ("mart_pollutant_trend; drop table x", "year"),
        ("mart_pollutant_trend", "year from x --"),
    ]:
        with pytest.raises(ValueError, match="not a mart column"):
            marts.distinct(mart, column)
        with pytest.raises(ValueError, match="not a mart column"):
            marts.date_range(mart, column)


def test_date_range_of_an_empty_mart_is_none(warehouse_path: Path) -> None:
    with duckdb.connect(str(warehouse_path)) as connection:
        connection.execute("delete from mart_traffic_vs_background")
    assert Marts(warehouse_path).date_range("mart_traffic_vs_background", "month") is None


def test_missing_warehouse_or_mart_is_a_readable_error(tmp_path: Path) -> None:
    with pytest.raises(MartsUnavailableError, match="make transform"):
        Marts(tmp_path / "missing.duckdb").exceedances("daily_mean")

    empty = tmp_path / "empty.duckdb"
    duckdb.connect(str(empty)).close()
    with pytest.raises(MartsUnavailableError, match="mart is missing"):
        Marts(empty).weather_correlation()


def test_the_warehouse_is_opened_read_only(warehouse_path: Path) -> None:
    marts = Marts(warehouse_path)
    with pytest.raises(duckdb.Error):
        marts._query("delete from mart_pollutant_trend")
    assert len(marts.pollutant_trend("PM10")) == 3


def test_warehouse_path_comes_from_the_environment(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("AQ_DUCKDB_PATH", raising=False)
    assert duckdb_path_from_env() == DEFAULT_DUCKDB_PATH
    monkeypatch.setenv("AQ_DUCKDB_PATH", "/data/custom.duckdb")
    assert duckdb_path_from_env() == Path("/data/custom.duckdb")


@contextmanager
def writer_holding(path: Path, seconds: float) -> Iterator[None]:
    """Another process keeping the warehouse open for writing, as a dbt build does."""
    script = (
        "import duckdb, sys, time\n"
        "connection = duckdb.connect(sys.argv[1])\n"
        "print('locked', flush=True)\n"
        "time.sleep(float(sys.argv[2]))\n"
    )
    process = subprocess.Popen(
        [sys.executable, "-c", script, str(path), str(seconds)],
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert process.stdout is not None
        assert process.stdout.readline().strip() == "locked"
        yield
    finally:
        process.kill()
        process.wait()


def test_a_query_waits_for_a_rebuild_to_finish(warehouse_path: Path) -> None:
    with writer_holding(warehouse_path, seconds=1.5):
        started = time.monotonic()
        frame = Marts(warehouse_path, lock_wait_seconds=10).pollutant_trend("PM10")

    assert len(frame) == 3
    assert time.monotonic() - started >= 1.0


def test_a_long_rebuild_is_reported_instead_of_crashing(warehouse_path: Path) -> None:
    with (
        writer_holding(warehouse_path, seconds=30),
        pytest.raises(MartsUnavailableError, match="being rebuilt"),
    ):
        Marts(warehouse_path, lock_wait_seconds=0.6).pollutant_trend("PM10")
