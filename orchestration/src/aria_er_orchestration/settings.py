"""Locations of the other components, resolved from the repository root.

Every path can be overridden with an environment variable, so the same code
runs from a checkout, a container or a test directory.
"""

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path


def _path(variable: str, default: Path) -> Path:
    value = os.environ.get(variable)
    return Path(value).resolve() if value else default


@dataclass(frozen=True)
class Settings:
    repo_root: Path
    ingest_binary: Path
    ingest_config: Path
    raw_dir: Path
    dbt_project_dir: Path
    duckdb_path: Path
    partitions_start: str

    @classmethod
    def from_env(cls) -> "Settings":
        root = _path("AQ_REPO_ROOT", Path(__file__).resolve().parents[3])
        return cls(
            repo_root=root,
            ingest_binary=_path("AQ_INGEST_BINARY", root / "ingestor/target/release/aq-ingest"),
            ingest_config=_path("AQ_CONFIG", root / "ingestor/config/default.toml"),
            raw_dir=_path("AQ_RAW_DIR", root / "raw"),
            dbt_project_dir=_path("AQ_DBT_PROJECT_DIR", root / "transform"),
            duckdb_path=_path("AQ_DUCKDB_PATH", root / "warehouse/aria_er.duckdb"),
            # First day held by the ARPAE near-real-time datastore when the project started.
            partitions_start=os.environ.get("AQ_PARTITIONS_START", "2026-07-28"),
        )

    def reprocess_window_days(self) -> int:
        """Days of provisional data reprocessed on every run, as configured for the ingestor."""
        with self.ingest_config.open("rb") as file:
            days = tomllib.load(file)["run"]["reprocess_window_days"]
        if not isinstance(days, int) or days < 1:
            raise ValueError(f"run.reprocess_window_days must be a positive integer, got {days!r}")
        return days


SETTINGS = Settings.from_env()
