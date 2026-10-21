"""Months between the end of the historical archive and today that the raw layer lacks.

The daily run fetches only the last 30 days, so whatever lies between the archive and that
window (on a first start, most of the current year) is never loaded. This module finds those
months from the raw layer itself, with no date written anywhere: when the archive grows, or a
new year starts, the windows follow. Run from `orchestration/`:

    uv run python -m aria_er_orchestration.gaps --raw ../raw --max-months 6

prints one `FROM TO` line per window, oldest first, for `aq-ingest run --from FROM --to TO`.
"""

import argparse
import calendar
from collections.abc import Sequence
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import duckdb

# ARPAE days are in UTC+1 all year, and every value is stamped at the end of its period.
ARPAE_OFFSET = timedelta(hours=1)
STAMP_TO_START = timedelta(seconds=1)


def legal_day(stamp: datetime) -> date:
    """The ARPAE day a value belongs to: the day its period starts in, in UTC+1."""
    return (stamp.astimezone(UTC) + ARPAE_OFFSET - STAMP_TO_START).date()


def today_in_arpae_time(now: datetime) -> date:
    return (now.astimezone(UTC) + ARPAE_OFFSET).date()


def _stamps(raw_dir: Path, source: str, query: str) -> list[datetime]:
    pattern = raw_dir / "arpae" / source / "*" / "*" / "*.parquet"
    if not any(raw_dir.glob(f"arpae/{source}/*/*/*.parquet")):
        return []
    with duckdb.connect() as connection:
        connection.execute("set TimeZone = 'UTC'")
        rows = connection.execute(query.format(files=f"read_parquet('{pattern}')")).fetchall()
    return [row[0] for row in rows if row[0] is not None]


def archive_end(raw_dir: Path) -> date | None:
    """Last day of the historical archive in the raw layer, or None when there is none."""
    stamps = _stamps(raw_dir, "measurements_archive", "select max(measured_at) from {files}")
    return legal_day(stamps[0]) if stamps else None


def loaded_days(raw_dir: Path) -> set[date]:
    """Days with at least one recent measurement in the raw layer."""
    stamps = _stamps(
        raw_dir,
        "measurements",
        # One stamp per hour is enough to know which days are there.
        "select distinct date_trunc('hour', measured_at) from {files}",
    )
    return {legal_day(stamp) for stamp in stamps}


def _months(since: date, today: date) -> list[tuple[date, date]]:
    """Calendar months from the month of `since` to that of `today`, clipped to both."""
    months: list[tuple[date, date]] = []
    year, month = since.year, since.month
    while (year, month) <= (today.year, today.month):
        last = date(year, month, calendar.monthrange(year, month)[1])
        months.append((max(date(year, month, 1), since), min(last, today)))
        year, month = (year + 1, 1) if month == 12 else (year, month + 1)
    return months


def missing_windows(
    *, since: date, today: date, loaded: set[date], max_months: int
) -> list[tuple[date, date]]:
    """Months to fetch, oldest first, at most `max_months`.

    A past month is loaded when it has values on its first and on its last day; the current
    one when it has values on its first day, since the last days are not published yet and
    the daily window covers them. Days missing in between are gaps of the source, which
    fetching again would not fill.
    """
    if max_months < 1:
        raise ValueError(f"max_months must be positive, got {max_months}")
    if since > today:
        return []
    missing = [
        (first, last)
        for first, last in _months(since, today)
        if first not in loaded or (last < today and last not in loaded)
    ]
    return missing[:max_months]


def main(argv: Sequence[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("--raw", type=Path, required=True, help="raw layer directory")
    parser.add_argument("--max-months", type=int, default=6, help="months per run")
    parser.add_argument(
        "--today", type=date.fromisoformat, help="ARPAE day to stop at (default: today)"
    )
    arguments = parser.parse_args(argv)

    today = arguments.today or today_in_arpae_time(datetime.now(UTC))
    end = archive_end(arguments.raw)
    # Without an archive, the current year: the archive is what covers earlier years.
    since = end + timedelta(days=1) if end else date(today.year, 1, 1)
    windows = missing_windows(
        since=since, today=today, loaded=loaded_days(arguments.raw), max_months=arguments.max_months
    )
    for first, last in windows:
        print(f"{first.isoformat()} {last.isoformat()}")


if __name__ == "__main__":
    main()
