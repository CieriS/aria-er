"""Daily JSON export of the marts, read by the portfolio case study (ADR 0009).

The contract is documented in docs/export.md. Four marts go out whole; the daily
completeness mart goes out per station, pollutant and month. Run from `orchestration/`:

    uv run python -m aria_er_orchestration.export --duckdb ../warehouse/aria_er.duckdb \
        --out ../site/data/v1
"""

import argparse
import json
import math
from collections.abc import Sequence
from datetime import UTC, date, datetime
from decimal import Decimal
from pathlib import Path
from typing import Any

import duckdb

SCHEMA_VERSION = 1
# Floats are rounded to this many decimals: enough for µg/m³ and shares, half the bytes.
DECIMALS = 3
# A file larger than this means the shape of a mart changed: stop instead of publishing it.
MAX_FILE_BYTES = 2_000_000

FILES: dict[str, str] = {
    "exceedances.json": """
        select station_id, station_name, municipality, pollutant_code, metric, year,
               exceedances, limit_ugm3, max_exceedances_per_year, is_over_allowed_exceedances,
               max_value_ugm3, valid_periods, year_coverage
        from mart_exceedances_yearly
        order by station_id, pollutant_code, metric, year
    """,
    "pollutant-trend.json": """
        select municipality, pollutant_code, year, stations, annual_mean_ugm3,
               min_station_mean_ugm3, max_station_mean_ugm3, min_year_coverage,
               is_representative
        from mart_pollutant_trend
        order by municipality, pollutant_code, year
    """,
    "traffic-vs-background.json": """
        select municipality, pollutant_code, month, days, traffic_stations,
               background_stations, traffic_mean_ugm3, background_mean_ugm3, difference_ugm3,
               traffic_to_background_ratio
        from mart_traffic_vs_background
        order by municipality, pollutant_code, month
    """,
    "weather-correlation.json": """
        select station_id, station_name, municipality, season, days, pm10_mean_ugm3,
               corr_pm10_wind, corr_pm10_precipitation, windy_days, pm10_mean_windy_ugm3,
               pm10_mean_calm_ugm3, rainy_days, pm10_mean_rainy_ugm3, pm10_mean_dry_ugm3
        from mart_weather_correlation
        order by station_id, season
    """,
    # Per month rather than per day: the daily mart has about 180,000 rows.
    "completeness-monthly.json": """
        select station_id, station_name, municipality, pollutant_code,
               cast(date_trunc('month', measurement_date) as date) as month,
               count(*) as days,
               count(*) filter (where is_complete) as complete_days,
               avg(completeness) as completeness
        from mart_data_completeness
        group by all
        order by station_id, pollutant_code, month
    """,
}

PERIOD_QUERY = "select min(measurement_date), max(measurement_date) from mart_data_completeness"

CREDITS = [
    {
        "source": "ARPAE Emilia-Romagna",
        "url": "https://dati.arpae.it",
        "licence": "CC BY",
        "note": "Air quality measurements, processed by aria-er.",
    },
    {
        "source": "Open-Meteo",
        "url": "https://open-meteo.com",
        "licence": "CC BY 4.0",
        "note": "Weather data.",
    },
]


class ExportTooLargeError(RuntimeError):
    """A file of the export is over MAX_FILE_BYTES."""


def to_json_value(value: object) -> object:
    """A value from DuckDB as plain JSON: ISO dates, rounded floats, null for NaN and inf."""
    if isinstance(value, bool) or value is None or isinstance(value, int | str):
        return value
    if isinstance(value, datetime | date):
        return value.isoformat()
    if isinstance(value, Decimal):
        value = float(value)
    if isinstance(value, float):
        return round(value, DECIMALS) if math.isfinite(value) else None
    raise TypeError(f"Unsupported value in the export: {value!r}")


def to_records(columns: Sequence[str], rows: Sequence[Sequence[object]]) -> list[dict[str, Any]]:
    return [
        {column: to_json_value(value) for column, value in zip(columns, row, strict=True)}
        for row in rows
    ]


def write_json(path: Path, payload: object) -> None:
    text = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    size = len(text.encode("utf-8"))
    if size > MAX_FILE_BYTES:
        raise ExportTooLargeError(f"{path.name} is {size} bytes, over {MAX_FILE_BYTES}")
    path.write_text(text, encoding="utf-8")


def export_marts(duckdb_path: Path, out_dir: Path, generated_at: datetime) -> dict[str, int]:
    """Writes one file per entry of FILES and `meta.json`; returns the rows of each file."""
    out_dir.mkdir(parents=True, exist_ok=True)
    counts: dict[str, int] = {}
    with duckdb.connect(str(duckdb_path), read_only=True) as connection:
        for name, query in FILES.items():
            cursor = connection.execute(query)
            columns = [column[0] for column in cursor.description]
            records = to_records(columns, cursor.fetchall())
            write_json(out_dir / name, records)
            counts[name] = len(records)
        first, last = connection.execute(PERIOD_QUERY).fetchone() or (None, None)

    period = None if first is None else {"from": to_json_value(first), "to": to_json_value(last)}
    meta = {
        "schema_version": SCHEMA_VERSION,
        "generated_at": generated_at.astimezone(UTC).isoformat(),
        "period": period,
        "files": counts,
        "credits": CREDITS,
    }
    write_json(out_dir / "meta.json", meta)
    return counts


def main(argv: Sequence[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0] if __doc__ else None)
    parser.add_argument("--duckdb", type=Path, required=True, help="warehouse file")
    parser.add_argument("--out", type=Path, required=True, help="directory of the export")
    arguments = parser.parse_args(argv)
    counts = export_marts(arguments.duckdb, arguments.out, datetime.now(UTC))
    for name, rows in counts.items():
        print(f"{name}: {rows} rows")


if __name__ == "__main__":
    main()
