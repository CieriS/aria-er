import json
import os
import subprocess
from dataclasses import dataclass
from datetime import date, datetime
from pathlib import Path
from typing import Any

import duckdb
from dagster import ConfigurableResource, Failure, MetadataValue

# Lines of the ingestor log shown when a run fails.
_ERROR_TAIL_LINES = 20
_SUMMARY_MESSAGE = "ingestion completed"
_SUMMARY_FIELDS = (
    "fetched",
    "inserted",
    "updated",
    "partitions_written",
    "rows_stored",
    "station_rows",
    "station_type_rows",
    "locations",
    "years",
)


@dataclass(frozen=True)
class IngestSummary:
    """Counters reported by `aq-ingest` at the end of a run."""

    counters: dict[str, int]
    log: str


class AqIngestResource(ConfigurableResource):  # type: ignore[type-arg]
    """Runs the `aq-ingest` binary. All ingestion logic lives in the Rust crate."""

    binary_path: str
    config_path: str
    working_dir: str

    def run(
        self, first_day: date | None, last_day: date | None, command: str = "run"
    ) -> IngestSummary:
        """Runs an `aq-ingest` subcommand on the inclusive window.

        `run` ingests ARPAE, `weather` ingests Open-Meteo, `archive` loads the local
        archive files. Without days the subcommand uses its own default window (for
        `archive`, everything). Raises `Failure` with the ingestor log on error.
        """
        argv = [
            self.binary_path,
            "--config",
            self.config_path,
            command,
        ]
        if first_day is not None:
            argv += ["--from", first_day.isoformat()]
        if last_day is not None:
            argv += ["--to", last_day.isoformat()]
        try:
            completed = subprocess.run(
                argv,
                cwd=self.working_dir,
                env={**os.environ, "AQ_LOG__FORMAT": "json"},
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            raise Failure(
                description=f"Cannot start {self.binary_path}: {error}. Run `make ingest` "
                "once to build the binary.",
            ) from error

        log = completed.stderr
        if completed.returncode != 0:
            tail = "\n".join(log.strip().splitlines()[-_ERROR_TAIL_LINES:])
            raise Failure(
                description=(
                    f"aq-ingest {command} failed with exit code {completed.returncode} "
                    f"for {first_day or 'start'}..{last_day or 'end'}"
                ),
                metadata={
                    "command": " ".join(argv),
                    "log_tail": MetadataValue.md(f"```\n{tail}\n```"),
                },
            )
        return IngestSummary(counters=_parse_summary(log), log=log)


def _parse_summary(log: str) -> dict[str, int]:
    """Extracts the counters of the final JSON log line; empty if it is not found."""
    for line in reversed(log.splitlines()):
        try:
            fields = json.loads(line).get("fields", {})
        except (ValueError, AttributeError):
            continue
        if fields.get("message") == _SUMMARY_MESSAGE:
            return {
                name: int(fields[name])
                for name in _SUMMARY_FIELDS
                if isinstance(fields.get(name), int)
            }
    return {}


class WarehouseResource(ConfigurableResource):  # type: ignore[type-arg]
    """Read-only queries used by the asset checks."""

    raw_dir: str
    duckdb_path: str

    def latest_measurement(self) -> datetime | None:
        """Newest `measured_at` in the raw layer (UTC), or None when it is empty."""
        pattern = Path(self.raw_dir) / "arpae/measurements/*/*/*.parquet"
        if not list(Path(self.raw_dir).glob("arpae/measurements/*/*/*.parquet")):
            return None
        with duckdb.connect() as connection:
            connection.execute("set TimeZone = 'UTC'")
            row = connection.execute(
                "select max(measured_at) from read_parquet(?)", [str(pattern)]
            ).fetchone()
        latest: datetime | None = row[0] if row else None
        return latest

    def completeness(self, days: int) -> tuple[int, int]:
        """(complete, total) station-pollutant-days over the last `days` days of the mart."""
        query = """
            select count(*) filter (where is_complete), count(*)
            from mart_data_completeness
            where measurement_date > (
                select max(measurement_date) from mart_data_completeness
            ) - cast(? as integer)
        """
        with duckdb.connect(self.duckdb_path, read_only=True) as connection:
            row: tuple[Any, ...] | None = connection.execute(query, [days]).fetchone()
        if row is None:
            return 0, 0
        return int(row[0]), int(row[1])
