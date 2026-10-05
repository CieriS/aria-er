import stat
from datetime import UTC, date, datetime
from pathlib import Path

import duckdb
import pytest
from dagster import AssetCheckResult, Failure

from aria_er_orchestration.checks import raw_measurements_freshness, recent_data_completeness
from aria_er_orchestration.resources import AqIngestResource, WarehouseResource

SUMMARY_LINE = (
    '{"timestamp":"2026-10-01T20:32:39Z","level":"INFO","fields":{"message":"ingestion completed",'
    '"from":"2026-08-01","to":"2026-08-31","station_rows":273,"station_type_rows":67,"fetched":112795,"inserted":12,'
    '"updated":3,"partitions_written":2,"rows_stored":112795},"target":"aq_ingest::pipeline"}'
)


def fake_binary(directory: Path, script: str) -> AqIngestResource:
    """A shell script standing in for `aq-ingest`."""
    binary = directory / "aq-ingest"
    binary.write_text(f"#!/bin/sh\n{script}\n")
    binary.chmod(binary.stat().st_mode | stat.S_IEXEC)
    return AqIngestResource(
        binary_path=str(binary), config_path="config.toml", working_dir=str(directory)
    )


def test_ingest_passes_the_window_and_parses_the_summary(tmp_path: Path) -> None:
    resource = fake_binary(
        tmp_path,
        f'echo "$@" > args.txt\necho \'{SUMMARY_LINE}\' >&2\necho "$AQ_LOG__FORMAT" > env.txt',
    )

    summary = resource.run(date(2026, 8, 1), date(2026, 8, 31))

    assert (tmp_path / "args.txt").read_text().split() == [
        "--config", "config.toml", "run", "--from", "2026-08-01", "--to", "2026-08-31",
    ]  # fmt: skip
    assert (tmp_path / "env.txt").read_text().strip() == "json"
    assert summary.counters == {
        "fetched": 112795,
        "inserted": 12,
        "updated": 3,
        "partitions_written": 2,
        "rows_stored": 112795,
        "station_rows": 273,
        "station_type_rows": 67,
    }


def test_ingest_can_run_the_weather_subcommand(tmp_path: Path) -> None:
    resource = fake_binary(tmp_path, 'echo "$@" > args.txt')

    resource.run(date(2026, 8, 1), date(2026, 8, 2), command="weather")

    assert (tmp_path / "args.txt").read_text().split()[2] == "weather"


def test_ingest_failure_carries_the_ingestor_log(tmp_path: Path) -> None:
    resource = fake_binary(
        tmp_path, "echo 'Error: fetching measurements: HTTP status 502' >&2\nexit 1"
    )

    with pytest.raises(Failure) as raised:
        resource.run(date(2026, 8, 1), date(2026, 8, 1))

    failure = raised.value
    assert "exit code 1" in str(failure.description)
    assert "HTTP status 502" in str(failure.metadata["log_tail"].value)


def test_ingest_missing_binary_is_a_readable_failure(tmp_path: Path) -> None:
    resource = AqIngestResource(
        binary_path=str(tmp_path / "missing"), config_path="c.toml", working_dir=str(tmp_path)
    )
    with pytest.raises(Failure, match="Cannot start"):
        resource.run(date(2026, 8, 1), date(2026, 8, 1))


def test_ingest_without_summary_line_reports_no_counters(tmp_path: Path) -> None:
    resource = fake_binary(tmp_path, "echo 'plain text log' >&2")
    assert resource.run(date(2026, 8, 1), date(2026, 8, 1)).counters == {}


def warehouse(tmp_path: Path, latest: datetime | None, complete_days: int) -> WarehouseResource:
    """A raw layer with one measurement and a mart with 10 recent days."""
    raw_dir = tmp_path / "raw"
    if latest is not None:
        partition = raw_dir / "arpae/measurements/year=2026/month=08"
        partition.mkdir(parents=True)
        with duckdb.connect() as connection:
            connection.execute("set TimeZone = 'UTC'")
            connection.execute(
                f"copy (select cast(? as timestamptz) as measured_at) "
                f"to '{partition / 'part-0.parquet'}' (format parquet)",
                [latest],
            )
    database = tmp_path / "warehouse.duckdb"
    with duckdb.connect(str(database)) as connection:
        connection.execute(
            "create table mart_data_completeness as "
            "select date '2026-08-31' - cast(i as integer) as measurement_date, "
            "i < ? as is_complete from range(40) as r(i)",
            [complete_days],
        )
    return WarehouseResource(raw_dir=str(raw_dir), duckdb_path=str(database))


def freshness(resource: WarehouseResource) -> AssetCheckResult:
    result = raw_measurements_freshness(resource)
    assert isinstance(result, AssetCheckResult)
    return result


def completeness(resource: WarehouseResource) -> AssetCheckResult:
    result = recent_data_completeness(resource)
    assert isinstance(result, AssetCheckResult)
    return result


def test_freshness_check_passes_on_recent_data(tmp_path: Path) -> None:
    result = freshness(warehouse(tmp_path, datetime.now(UTC), 30))
    assert result.passed


def test_freshness_check_fails_on_stale_or_missing_data(tmp_path: Path) -> None:
    stale = freshness(warehouse(tmp_path / "a", datetime(2026, 8, 31, tzinfo=UTC), 30))
    assert not stale.passed
    assert stale.metadata["latest_measurement_utc"].value == "2026-08-31T00:00:00+00:00"

    (tmp_path / "b").mkdir()
    assert not freshness(warehouse(tmp_path / "b", None, 30)).passed


def test_completeness_check_uses_only_the_recent_window(tmp_path: Path) -> None:
    # 40 days in the mart, the 30 most recent all complete, the older 10 not.
    result = completeness(warehouse(tmp_path, None, 30))
    assert result.passed
    assert result.metadata["total_days"].value == 30


def test_completeness_check_fails_below_the_threshold(tmp_path: Path) -> None:
    result = completeness(warehouse(tmp_path, None, 20))
    assert not result.passed
    assert result.metadata["complete_days"].value == 20
