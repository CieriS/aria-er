"""Rebuild the dbt fixtures from the local raw layer and the archive samples.

The fixtures are a small real slice (the Bologna stations, ten days of August 2026,
plus PM10 of 2025 for two stations from the archive) with exactly the schema the ingestor writes, so that
`dbt build` can run without network or a previous ingestion. Run from `transform/`
after `make ingest` has populated `../raw` (archive included):

    uv run python fixtures/build_fixtures.py
"""

import shutil
from pathlib import Path

import duckdb

HERE = Path(__file__).resolve().parent
RAW = HERE.parent.parent / "raw"
OUT_RAW = HERE / "raw"

STATIONS = "(7000014, 7000015, 7000041)"
FIRST_DAY, LAST_DAY = "2026-08-01", "2026-08-11"


def copy(connection: duckdb.DuckDBPyConnection, query: str, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    connection.execute(f"copy ({query}) to '{target}' (format parquet)")
    rows = connection.execute(f"select count(*) from '{target}'").fetchone()
    print(f"{target.relative_to(HERE)}: {rows[0] if rows else 0} rows")


def main() -> None:
    shutil.rmtree(OUT_RAW, ignore_errors=True)

    connection = duckdb.connect()
    connection.execute("set TimeZone = 'UTC'")
    window = f"between timestamptz '{FIRST_DAY}' and timestamptz '{LAST_DAY}'"

    copy(
        connection,
        f"select * exclude (year, month) from read_parquet('{RAW}/arpae/measurements/*/*/*.parquet', "
        f"hive_partitioning = true) where station_id in {STATIONS} and measured_at {window} "
        "order by station_id, pollutant_id, measured_at",
        OUT_RAW / "arpae/measurements/year=2026/month=08/part-0.parquet",
    )
    latest = "extracted_on = (select max(extracted_on) from source)"
    copy(
        connection,
        f"with source as (select * from read_parquet('{RAW}/arpae/stations/*/*.parquet')) "
        f"select * from source where {latest} and station_id in {STATIONS} "
        "order by station_id, pollutant_id",
        OUT_RAW / "arpae/stations/extracted_on=2026-10-05/stations.parquet",
    )
    copy(
        connection,
        f"with source as (select * from read_parquet('{RAW}/arpae/station_types/*/*.parquet')) "
        f"select * from source where {latest} and station_id in {STATIONS} order by station_id",
        OUT_RAW / "arpae/station_types/extracted_on=2026-10-05/station_types.parquet",
    )
    # Weather locations of the three stations (coordinates rounded to one decimal).
    copy(
        connection,
        f"select * exclude (year, month) from read_parquet('{RAW}/openmeteo/weather/*/*/*.parquet', "
        f"hive_partitioning = true) where location_id in ('445_114', '445_113') "
        f"and observed_at {window} order by location_id, observed_at",
        OUT_RAW / "openmeteo/weather/year=2026/month=08/part-0.parquet",
    )

    # PM10 of 2025 for two stations: enough for a yearly exceedance count.
    copy(
        connection,
        f"select * exclude (year, month) from read_parquet('{RAW}/arpae/measurements_archive/*/*/*.parquet', "
        "hive_partitioning = true) where station_id in (7000014, 7000015) and pollutant_id = 5 "
        "and measured_at between timestamptz '2025-01-01' and timestamptz '2025-12-31' "
        "order by station_id, pollutant_id, measured_at",
        OUT_RAW / "arpae/measurements_archive/year=2025/month=01/part-0.parquet",
    )


if __name__ == "__main__":
    main()
