from datetime import date
from pathlib import Path

from aria_er_orchestration.definitions import defs
from aria_er_orchestration.schedules import (
    PARTITION_RANGE_END_TAG,
    PARTITION_RANGE_START_TAG,
    bootstrap_requests,
)

TODAY = date(2026, 10, 7)


def with_archive(raw_dir: Path) -> None:
    partition = raw_dir / "arpae/measurements_archive/year=2025/month=01"
    partition.mkdir(parents=True)
    (partition / "part-0.parquet").touch()


def test_empty_installation_loads_the_archive_first(tmp_path: Path) -> None:
    requests = bootstrap_requests(tmp_path / "raw", tmp_path / "warehouse.duckdb", TODAY)

    assert [request.job_name for request in requests] == ["arpae_archive_load"]
    assert requests[0].run_key == "bootstrap-archive-2026-10-07"


def test_with_the_archive_loaded_it_requests_the_provisional_window(tmp_path: Path) -> None:
    with_archive(tmp_path / "raw")

    requests = bootstrap_requests(tmp_path / "raw", tmp_path / "warehouse.duckdb", TODAY)

    assert [request.job_name for request in requests] == ["arpae_refresh"]
    assert requests[0].run_key == "bootstrap-refresh-2026-10-07"
    assert requests[0].tags[PARTITION_RANGE_START_TAG] == "2026-09-08"
    assert requests[0].tags[PARTITION_RANGE_END_TAG] == "2026-10-07"


def test_an_existing_warehouse_needs_nothing(tmp_path: Path) -> None:
    warehouse = tmp_path / "warehouse.duckdb"
    warehouse.touch()

    assert bootstrap_requests(tmp_path / "raw", warehouse, TODAY) == []
    with_archive(tmp_path / "raw")
    assert bootstrap_requests(tmp_path / "raw", warehouse, TODAY) == []


def test_each_step_keeps_the_same_run_key_within_a_day(tmp_path: Path) -> None:
    # Dagster skips a run key it has already launched: a step still running, or failed,
    # is not launched again every minute.
    first = bootstrap_requests(tmp_path / "raw", tmp_path / "warehouse.duckdb", TODAY)
    again = bootstrap_requests(tmp_path / "raw", tmp_path / "warehouse.duckdb", TODAY)
    assert first[0].run_key == again[0].run_key

    tomorrow = bootstrap_requests(
        tmp_path / "raw", tmp_path / "warehouse.duckdb", date(2026, 10, 8)
    )
    assert tomorrow[0].run_key != first[0].run_key


def test_sensor_is_on_by_default_and_targets_both_jobs() -> None:
    sensor = defs.resolve_sensor_def("bootstrap_empty_warehouse")
    assert sensor.default_status.value == "RUNNING"
    assert {target.job_name for target in sensor.targets} == {
        "arpae_archive_load",
        "arpae_refresh",
    }
