"""Compare a mart between the local DuckDB warehouse and BigQuery.

Exits with 1 and prints the differing rows when the two do not hold the same data.
Run from `transform/` with the cloud dependencies and GCP credentials:

    AQ_GCP_PROJECT=<project> uv run --group cloud python scripts/compare_targets.py

Floating point columns are compared after rounding to six decimals.
"""

import argparse
import os
import sys
from collections.abc import Iterable, Sequence
from decimal import Decimal
from pathlib import Path

import duckdb

DEFAULT_DUCKDB = Path(__file__).resolve().parents[2] / "warehouse" / "aria_er.duckdb"
FLOAT_DECIMALS = 6


def normalise(row: Sequence[object]) -> tuple[object, ...]:
    """A row with numbers in a form comparable across the two engines."""
    values: list[object] = []
    for value in row:
        if isinstance(value, bool) or value is None:
            values.append(value)
        elif isinstance(value, (float, Decimal)):
            values.append(round(float(value), FLOAT_DECIMALS))
        else:
            values.append(value)
    return tuple(values)


def difference(
    local: Iterable[Sequence[object]], cloud: Iterable[Sequence[object]]
) -> tuple[list[tuple[object, ...]], list[tuple[object, ...]]]:
    """Rows only in the local result and rows only in the cloud result."""
    local_rows = {normalise(row) for row in local}
    cloud_rows = {normalise(row) for row in cloud}
    return sorted(local_rows - cloud_rows, key=repr), sorted(cloud_rows - local_rows, key=repr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--mart", default="mart_exceedances_yearly")
    parser.add_argument("--duckdb", type=Path, default=DEFAULT_DUCKDB)
    parser.add_argument("--project", default=os.environ.get("AQ_GCP_PROJECT"))
    parser.add_argument("--dataset", default=os.environ.get("AQ_BIGQUERY_DATASET", "aria_er"))
    arguments = parser.parse_args()
    if not arguments.mart.isidentifier():
        parser.error("--mart must be a table name")
    if not arguments.project:
        parser.error("set AQ_GCP_PROJECT or pass --project")

    with duckdb.connect(str(arguments.duckdb), read_only=True) as connection:
        description = connection.execute(f"describe {arguments.mart}").fetchall()
        columns = [row[0] for row in description]
        local = connection.execute(f"select {', '.join(columns)} from {arguments.mart}").fetchall()

    # Imported here so that the pure functions above can be used without the cloud group.
    from google.cloud import bigquery

    client = bigquery.Client(project=arguments.project)
    table = f"`{arguments.project}.{arguments.dataset}.{arguments.mart}`"
    cloud = [tuple(row.values()) for row in client.query(f"select {', '.join(columns)} from {table}")]

    only_local, only_cloud = difference(local, cloud)
    print(f"{arguments.mart}: {len(local)} rows in DuckDB, {len(cloud)} rows in BigQuery")
    if not only_local and not only_cloud:
        print("identical")
        return 0
    for label, rows in (("only in DuckDB", only_local), ("only in BigQuery", only_cloud)):
        print(f"{label}: {len(rows)}")
        for row in rows[:20]:
            print("  ", row)
    return 1


if __name__ == "__main__":
    sys.exit(main())
