"""Check the BigQuery side of the dbt project without a GCP project.

Runs against the open-source BigQuery emulator (ghcr.io/goccy/bigquery-emulator), whose
SQL front end is ZetaSQL, the analyser BigQuery itself uses:

1. loads the dbt fixtures into the emulator as the tables of the raw dataset;
2. compiles the project for the BigQuery adapter;
3. executes every model, in dependency order, and every error-level test;
4. compiles and executes the incremental model again, now that its table exists;
5. compares the results with the DuckDB build of the same fixtures.

Start the emulator and build the fixtures first (`make bigquery-dialect-check` does both):

    uv run --group cloud python scripts/check_bigquery_dialect.py --duckdb /tmp/aria_er_fixtures.duckdb

What it does not cover: dbt's own BigQuery materialisations (merge, snapshot, seed load),
external tables over Cloud Storage, and the execution of the RANGE window frame of the
8-hour mean, which the emulator's SQLite back end cannot run (its SQL is still analysed).
"""

import argparse
import csv
import datetime as dt
import glob
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

import duckdb
from google.api_core.client_options import ClientOptions
from google.auth.credentials import AnonymousCredentials
from google.cloud import bigquery

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compare_targets import difference  # noqa: E402

TRANSFORM = Path(__file__).resolve().parents[1]
SCHEMAS = TRANSFORM.parent / "infra" / "schemas"
RAW_DATASET, DATASET = "aria_er_raw", "aria_er"

# Raw tables as Terraform creates them: fixture files and the schema of the external table.
RAW_TABLES = {
    "arpae_measurements": ("arpae/measurements/*/*/*.parquet", "measurements"),
    "arpae_measurements_archive": ("arpae/measurements_archive/*/*/*.parquet", "measurements"),
    "openmeteo_weather": ("openmeteo/weather/*/*/*.parquet", "weather"),
    "arpae_stations": ("arpae/stations/*/*.parquet", "stations"),
    "arpae_station_types": ("arpae/station_types/*/*.parquet", "station_types"),
}
# Model whose window frame the emulator analyses but cannot execute, with the columns of
# the empty table that stands in for it so that the models downstream are still checked.
NOT_EXECUTABLE = {
    "int_o3_8h_rolling": (
        "station_id int64, pollutant_id int64, pollutant_code string, "
        "measured_at_utc datetime, measurement_date date, rolling_8h_mean_ugm3 float64, "
        "valid_hours_in_window int64, is_valid_window bool"
    )
}
# Results compared with DuckDB, and the rows left out because of the stand-in above.
COMPARED = {
    "int_measurements_deduplicated": "true",
    "int_measurements_daily": "true",
    "int_weather_daily": "true",
    "mart_exceedances_yearly": "metric <> 'daily_max_8h_mean'",
    "mart_pollutant_trend": "true",
    "mart_traffic_vs_background": "true",
    "mart_weather_correlation": "true",
    "mart_data_completeness": "true",
}
INCREMENTAL_MODEL = "int_measurements_deduplicated"


class Emulator:
    def __init__(self, endpoint: str, project: str) -> None:
        self.project = project
        self.client = bigquery.Client(
            project=project,
            credentials=AnonymousCredentials(),
            client_options=ClientOptions(api_endpoint=endpoint),
        )

    def table(self, dataset: str, name: str) -> str:
        return f"`{self.project}.{dataset}.{name}`"

    def run(self, sql: str) -> list[Any]:
        # No retries: the client would otherwise retry a failing statement for minutes.
        job = self.client.query(sql, retry=None, job_retry=None)
        return list(job.result(timeout=300, retry=None))

    def insert(self, dataset: str, name: str, schema: list[bigquery.SchemaField], rows: list[dict[str, Any]]) -> None:
        reference = f"{self.project}.{dataset}.{name}"
        self.client.create_table(bigquery.Table(reference, schema=schema))
        for start in range(0, len(rows), 500):
            errors = self.client.insert_rows_json(reference, rows[start : start + 500])
            if errors:
                raise RuntimeError(f"loading {name}: {errors[:1]}")


def as_json(value: Any) -> Any:
    if isinstance(value, dt.datetime):
        return value.astimezone(dt.UTC).strftime("%Y-%m-%d %H:%M:%S+00:00")
    if isinstance(value, dt.date):
        return value.isoformat()
    return value


def load_raw(emulator: Emulator) -> None:
    connection = duckdb.connect()
    connection.execute("set TimeZone = 'UTC'")
    for name, (pattern, schema_name) in RAW_TABLES.items():
        fields = json.loads((SCHEMAS / f"{schema_name}.json").read_text())
        schema = [bigquery.SchemaField(f["name"], f["type"], mode=f["mode"]) for f in fields]
        columns = [field.name for field in schema]
        files = TRANSFORM / "fixtures" / "raw" / pattern
        rows = connection.execute(
            f"select {', '.join(columns)} from read_parquet('{files}', hive_partitioning = false)"
        ).fetchall()
        emulator.insert(RAW_DATASET, name, schema, [dict(zip(columns, map(as_json, row))) for row in rows])
        print(f"raw   {name}: {len(rows)} rows")
    emulator.client.create_dataset(f"{emulator.project}.{DATASET}", exists_ok=True)


def load_seeds(emulator: Emulator, nodes: dict[str, Any]) -> None:
    """Seeds are loaded here: the emulator rejects the load job dbt would use."""
    kinds = {"integer": "INT64", "float64": "FLOAT64", "string": "STRING"}
    casts = {"INT64": int, "FLOAT64": float, "STRING": str}
    for node in nodes.values():
        if node["resource_type"] != "seed":
            continue
        with (TRANSFORM / node["original_file_path"]).open(newline="") as file:
            rows = list(csv.DictReader(file))
        declared = node["config"].get("column_types", {})

        def kind(column: str) -> str:
            if column in declared:
                return kinds[declared[column].lower()]
            values = [row[column] for row in rows if row[column] != ""]
            for candidate, cast in (("INT64", int), ("FLOAT64", float)):
                try:
                    [cast(value) for value in values]
                    return candidate
                except ValueError:
                    continue
            return "STRING"

        types = {column: kind(column) for column in rows[0]}
        payload = [
            {c: (casts[t](row[c]) if row[c] != "" else None) for c, t in types.items()} for row in rows
        ]
        emulator.insert(DATASET, node["name"], [bigquery.SchemaField(c, t) for c, t in types.items()], payload)
        print(f"seed  {node['name']}: {len(payload)} rows")


def compile_project(endpoint: str, project: str, target_path: str) -> dict[str, Any]:
    """`dbt compile` for the BigQuery adapter, connected to the emulator."""
    import dbt.adapters.bigquery.credentials as credentials
    from dbt.cli.main import dbtRunner

    # The emulator takes no credentials and does not list routines.
    credentials._create_google_credentials = lambda _credentials: AnonymousCredentials()
    credentials._create_bigquery_defaults = lambda scopes=None: (AnonymousCredentials(), project)
    bigquery.Client.list_routines = lambda self, *args, **kwargs: iter(())

    with tempfile.TemporaryDirectory() as profiles:
        Path(profiles, "profiles.yml").write_text(
            "aria_er:\n  target: emulator\n  outputs:\n    emulator:\n      type: bigquery\n"
            f"      method: oauth\n      project: {project}\n      dataset: {DATASET}\n"
            f"      api_endpoint: {endpoint}\n      threads: 1\n"
        )
        arguments = ["compile", "--project-dir", str(TRANSFORM), "--profiles-dir", profiles]
        arguments += ["--target-path", target_path, "--quiet"]
        dbtRunner().invoke(arguments)
    manifest = json.loads(Path(target_path, "manifest.json").read_text())
    return {key: node for key, node in manifest["nodes"].items() if node["package_name"] == "aria_er"}


def compiled_sql(target_path: str, node: dict[str, Any]) -> str:
    if node["resource_type"] == "test":
        matches = glob.glob(f"{target_path}/compiled/aria_er/**/{os.path.basename(node['path'])}", recursive=True)
        if not matches:
            raise FileNotFoundError(f"no compiled SQL for test {node['name']}")
        return Path(matches[0]).read_text()
    return Path(target_path, "compiled", "aria_er", node["original_file_path"]).read_text()


def build_models(emulator: Emulator, nodes: dict[str, Any], target_path: str) -> list[str]:
    problems: list[str] = []
    pending = {k: n for k, n in nodes.items() if n["resource_type"] in ("model", "snapshot")}
    broken: set[str] = set()
    while pending:
        ready = sorted(k for k, n in pending.items() if not any(d in pending for d in n["depends_on"]["nodes"]))
        for key in ready:
            node = pending.pop(key)
            name, table = node["name"], emulator.table(DATASET, node["name"])
            if any(dependency in broken for dependency in node["depends_on"]["nodes"]):
                broken.add(key)
                problems.append(f"{name}: skipped, an upstream model failed")
                continue
            if node["resource_type"] == "snapshot":
                # A first snapshot is the source rows plus the validity columns.
                source = emulator.table(DATASET, "stg_arpae__stations")
                sql = f"select *, current_datetime() as dbt_valid_from, cast(null as datetime) as dbt_valid_to from {source}"
            else:
                sql = compiled_sql(target_path, node)
            try:
                emulator.run(f"create table {table} as\n{sql}")
                print(f"model {name}: {emulator.run(f'select count(*) from {table}')[0][0]} rows")
            except Exception as error:  # noqa: BLE001 - every failure is reported
                if name in NOT_EXECUTABLE and "failed to exec" in str(error):
                    emulator.run(f"create table {table} ({NOT_EXECUTABLE[name]})")
                    print(f"model {name}: analysed only (the emulator cannot execute its window frame)")
                else:
                    broken.add(key)
                    problems.append(f"{name}: {str(error)[-400:]}")
    return problems


def run_tests(emulator: Emulator, nodes: dict[str, Any], target_path: str) -> list[str]:
    problems, executed = [], 0
    for _, node in sorted(nodes.items()):
        if node["resource_type"] != "test" or node["config"].get("severity", "error").lower() == "warn":
            continue
        try:
            failures = emulator.run(f"select count(*) from (\n{compiled_sql(target_path, node)}\n)")[0][0]
            executed += 1
            if failures:
                problems.append(f"test {node['name']}: {failures} failing rows")
        except Exception as error:  # noqa: BLE001
            problems.append(f"test {node['name']}: {str(error)[-300:]}")
    print(f"tests {executed} executed")
    return problems


def compare(emulator: Emulator, duckdb_path: Path) -> list[str]:
    problems = []
    with duckdb.connect(str(duckdb_path), read_only=True) as connection:
        for name, condition in COMPARED.items():
            columns = ", ".join(row[0] for row in connection.execute(f"describe {name}").fetchall())
            local = connection.execute(f"select {columns} from {name} where {condition}").fetchall()
            cloud = [tuple(row.values()) for row in emulator.run(f"select {columns} from {emulator.table(DATASET, name)} where {condition}")]
            only_local, only_cloud = difference(local, cloud)
            if only_local or only_cloud:
                problems.append(f"{name}: {len(only_local)} rows only in DuckDB, {len(only_cloud)} only in BigQuery")
            else:
                print(f"same  {name}: {len(local)} rows")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--endpoint", default="http://localhost:9050")
    parser.add_argument("--project", default="aria-er-emulator")
    parser.add_argument("--duckdb", type=Path, required=True, help="DuckDB build of the same fixtures")
    arguments = parser.parse_args()
    emulator = Emulator(arguments.endpoint, arguments.project)

    problems: list[str] = []
    with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
        load_raw(emulator)
        nodes = compile_project(arguments.endpoint, arguments.project, first)
        load_seeds(emulator, nodes)
        problems += build_models(emulator, nodes, first)
        problems += run_tests(emulator, nodes, first)

        # The table exists now, so this compilation takes the incremental branch.
        incremental = compile_project(arguments.endpoint, arguments.project, second)
        node = next(n for n in incremental.values() if n["name"] == INCREMENTAL_MODEL)
        sql = compiled_sql(second, node)
        if "loaded.source_name" not in sql:
            problems.append(f"{INCREMENTAL_MODEL}: the second compilation is not incremental")
        try:
            rows = emulator.run(f"select count(*) from (\n{sql}\n)")[0][0]
            print(f"model {INCREMENTAL_MODEL} (incremental): {rows} rows selected")
        except Exception as error:  # noqa: BLE001
            problems.append(f"{INCREMENTAL_MODEL} (incremental): {str(error)[-400:]}")

        problems += compare(emulator, arguments.duckdb)

    for problem in problems:
        print("PROBLEM", problem)
    print("BigQuery dialect check:", "failed" if problems else "passed")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
