from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import duckdb
import pytest

from aria_er_orchestration.gaps import (
    archive_end,
    legal_day,
    loaded_days,
    main,
    missing_windows,
    weather_days,
)

ARPAE = timedelta(hours=1)


def write_measurements(raw: Path, source: str, stamps: list[datetime]) -> None:
    """One partition file per (year, month) of the stamps, with the columns the module reads."""
    by_month: dict[tuple[int, int], list[datetime]] = {}
    for stamp in stamps:
        by_month.setdefault((stamp.year, stamp.month), []).append(stamp)
    with duckdb.connect() as connection:
        connection.execute("set TimeZone = 'UTC'")
        for (year, month), values in by_month.items():
            folder = raw / "arpae" / source / f"year={year}" / f"month={month:02d}"
            folder.mkdir(parents=True)
            rows = ", ".join(f"(timestamptz '{value.isoformat()}')" for value in values)
            connection.execute(
                f"copy (select * from (values {rows}) t(measured_at)) "
                f"to '{folder / 'part-0.parquet'}' (format parquet)"
            )


def hourly(first: date, last: date) -> list[datetime]:
    """Every hour of the legal days first..last, stamped at the end of the hour in UTC+1."""
    start = datetime(first.year, first.month, first.day, 1, tzinfo=UTC) - ARPAE
    hours = ((last - first).days + 1) * 24
    return [start + timedelta(hours=hour) for hour in range(hours)]


def test_a_value_belongs_to_the_day_its_hour_starts_in() -> None:
    # 00:00 of 1 January in UTC+1 closes the last hour of 31 December.
    assert legal_day(datetime(2025, 12, 31, 23, 0, tzinfo=UTC)) == date(2025, 12, 31)
    assert legal_day(datetime(2025, 12, 31, 23, 0, 1, tzinfo=UTC)) == date(2026, 1, 1)
    # A daily value stamped at the end of its day.
    assert legal_day(datetime(2026, 3, 1, 23, 0, tzinfo=UTC)) == date(2026, 3, 1)


def test_the_gap_starts_the_day_after_the_archive(tmp_path: Path) -> None:
    write_measurements(
        tmp_path, "measurements_archive", hourly(date(2025, 12, 30), date(2025, 12, 31))
    )
    assert archive_end(tmp_path) == date(2025, 12, 31)


def test_no_archive_means_no_known_end(tmp_path: Path) -> None:
    assert archive_end(tmp_path) is None


def test_loaded_days_are_the_legal_days_with_any_value(tmp_path: Path) -> None:
    write_measurements(tmp_path, "measurements", hourly(date(2026, 1, 31), date(2026, 2, 1)))
    assert loaded_days(tmp_path) == {date(2026, 1, 31), date(2026, 2, 1)}


def test_every_month_after_the_archive_is_missing_when_nothing_is_loaded() -> None:
    windows = missing_windows(
        since=date(2026, 1, 1), today=date(2026, 3, 10), loaded=set(), max_months=12
    )
    assert windows == [
        (date(2026, 1, 1), date(2026, 1, 31)),
        (date(2026, 2, 1), date(2026, 2, 28)),
        (date(2026, 3, 1), date(2026, 3, 10)),
    ]


def test_a_month_loaded_from_first_to_last_day_is_complete() -> None:
    january = {date(2026, 1, 1) + timedelta(days=day) for day in range(31)}
    windows = missing_windows(
        since=date(2026, 1, 1), today=date(2026, 2, 20), loaded=january, max_months=12
    )
    assert windows == [(date(2026, 2, 1), date(2026, 2, 20))]


def test_a_month_with_only_its_tail_loaded_is_missing() -> None:
    # The first daily run loads the last 30 days: the previous month is partial.
    tail = {date(2026, 9, 21) + timedelta(days=day) for day in range(31)}
    windows = missing_windows(
        since=date(2026, 9, 1), today=date(2026, 10, 21), loaded=tail, max_months=12
    )
    assert windows == [(date(2026, 9, 1), date(2026, 9, 30))]


def test_the_current_month_counts_when_it_is_loaded_from_its_first_day() -> None:
    october = {date(2026, 10, 1) + timedelta(days=day) for day in range(21)}
    windows = missing_windows(
        since=date(2026, 10, 1), today=date(2026, 10, 21), loaded=october, max_months=12
    )
    assert windows == []


def test_a_gap_inside_a_month_is_not_a_backfill() -> None:
    # Days missing in the middle are ARPAE's gaps, not months that were never loaded.
    days = {date(2026, 1, 1), date(2026, 1, 31)}
    windows = missing_windows(
        since=date(2026, 1, 1),
        today=date(2026, 2, 1),
        loaded=days | {date(2026, 2, 1)},
        max_months=12,
    )
    assert windows == []


def test_the_oldest_months_come_first_and_a_run_takes_at_most_max_months() -> None:
    windows = missing_windows(
        since=date(2026, 1, 15), today=date(2026, 12, 31), loaded=set(), max_months=2
    )
    assert windows == [
        (date(2026, 1, 15), date(2026, 1, 31)),
        (date(2026, 2, 1), date(2026, 2, 28)),
    ]


def test_it_works_across_a_new_year() -> None:
    windows = missing_windows(
        since=date(2026, 12, 1), today=date(2027, 1, 5), loaded=set(), max_months=12
    )
    assert windows == [
        (date(2026, 12, 1), date(2026, 12, 31)),
        (date(2027, 1, 1), date(2027, 1, 5)),
    ]


def test_nothing_is_missing_when_the_archive_reaches_today() -> None:
    assert missing_windows(since=date(2026, 10, 22), today=date(2026, 10, 21), loaded=set(),
                           max_months=12) == []  # fmt: skip


def test_a_non_positive_limit_is_refused() -> None:
    with pytest.raises(ValueError, match="max_months"):
        missing_windows(since=date(2026, 1, 1), today=date(2026, 2, 1), loaded=set(), max_months=0)


def test_the_command_prints_one_window_per_line(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    write_measurements(
        tmp_path, "measurements_archive", hourly(date(2025, 12, 31), date(2025, 12, 31))
    )
    write_measurements(tmp_path, "measurements", hourly(date(2026, 2, 1), date(2026, 2, 10)))
    write_weather(tmp_path, [date(2026, 2, 1) + timedelta(days=day) for day in range(10)])

    main(["--raw", str(tmp_path), "--today", "2026-02-10", "--max-months", "6"])

    assert capsys.readouterr().out.splitlines() == ["2026-01-01 2026-01-31"]


def test_without_an_archive_the_command_starts_from_the_current_year(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    main(["--raw", str(tmp_path), "--today", "2027-02-03", "--max-months", "6"])

    assert capsys.readouterr().out.splitlines() == [
        "2027-01-01 2027-01-31",
        "2027-02-01 2027-02-03",
    ]


def test_the_current_month_needs_only_its_first_day_since_today_is_not_published_yet() -> None:
    until_yesterday = {date(2026, 10, 1) + timedelta(days=day) for day in range(20)}
    windows = missing_windows(
        since=date(2026, 10, 1), today=date(2026, 10, 21), loaded=until_yesterday, max_months=12
    )
    assert windows == []


def write_weather(raw: Path, days: list[date]) -> None:
    """Hourly weather rows, stamped at the start of the hour in UTC, as Open-Meteo gives them."""
    with duckdb.connect() as connection:
        connection.execute("set TimeZone = 'UTC'")
        by_month: dict[tuple[int, int], list[date]] = {}
        for day in days:
            by_month.setdefault((day.year, day.month), []).append(day)
        for (year, month), values in by_month.items():
            folder = raw / "openmeteo" / "weather" / f"year={year}" / f"month={month:02d}"
            folder.mkdir(parents=True)
            rows = ", ".join(
                f"(timestamptz '{day.isoformat()}T{hour:02d}:00:00+00:00')"
                for day in values
                for hour in range(24)
            )
            connection.execute(
                f"copy (select * from (values {rows}) t(observed_at)) "
                f"to '{folder / 'part-0.parquet'}' (format parquet)"
            )


def test_weather_days_are_utc_days_with_any_value(tmp_path: Path) -> None:
    write_weather(tmp_path, [date(2026, 2, 1), date(2026, 2, 28)])
    assert weather_days(tmp_path) == {date(2026, 2, 1), date(2026, 2, 28)}


def test_a_month_with_measurements_but_no_weather_is_missing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    # Seen on 2026-10-21: a run fetched February's measurements, then failed on its weather.
    write_measurements(
        tmp_path, "measurements_archive", hourly(date(2025, 12, 31), date(2025, 12, 31))
    )
    write_measurements(tmp_path, "measurements", hourly(date(2026, 1, 1), date(2026, 3, 5)))
    january = [date(2026, 1, 1) + timedelta(days=day) for day in range(31)]
    march = [date(2026, 3, 1) + timedelta(days=day) for day in range(5)]
    write_weather(tmp_path, january + march)

    main(["--raw", str(tmp_path), "--today", "2026-03-05", "--max-months", "6"])

    assert capsys.readouterr().out.splitlines() == ["2026-02-01 2026-02-28"]
