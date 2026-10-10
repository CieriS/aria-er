import json
from collections.abc import Iterator
from datetime import date, timedelta
from typing import Any

from dagster import (
    AssetExecutionContext,
    AssetKey,
    AssetSpec,
    BackfillPolicy,
    Backoff,
    MaterializeResult,
    RetryPolicy,
    asset,
    multi_asset,
)
from dagster_dbt import DbtCliResource, DbtProject, dbt_assets

from aria_er_orchestration.partitions import daily_partitions
from aria_er_orchestration.resources import AqIngestResource
from aria_er_orchestration.settings import SETTINGS

# Same keys dagster-dbt gives to the dbt sources, so the lineage connects.
RAW_MEASUREMENTS = AssetKey(["arpae_raw", "measurements"])
RAW_ARCHIVE = AssetKey(["arpae_raw", "measurements_archive"])
RAW_STATIONS = AssetKey(["arpae_raw", "stations"])
RAW_STATION_TYPES = AssetKey(["arpae_raw", "station_types"])
RAW_WEATHER = AssetKey(["openmeteo_raw", "weather"])

dbt_project = DbtProject(
    project_dir=SETTINGS.dbt_project_dir,
    profiles_dir=SETTINGS.dbt_project_dir,
)
# `dagster dev` regenerates the manifest on load; elsewhere build it only if missing.
dbt_project.prepare_if_dev()
if not dbt_project.manifest_path.exists():
    dbt_project.preparer.prepare(dbt_project)


# The upstream APIs fail intermittently beyond what the ingestor's own retries cover.
INGEST_RETRY_POLICY = RetryPolicy(max_retries=2, delay=60, backoff=Backoff.EXPONENTIAL)


def _ingest_window(context: AssetExecutionContext) -> tuple[date, date]:
    """First and last day (inclusive) of the partitions selected for the run."""
    window = context.partition_time_window
    return window.start.date(), (window.end - timedelta(days=1)).date()


@multi_asset(
    specs=[
        AssetSpec(
            RAW_MEASUREMENTS,
            description="ARPAE near-real-time measurements as partitioned Parquet (raw layer).",
            group_name="ingestion",
            kinds={"rust", "parquet"},
        ),
        AssetSpec(
            RAW_STATIONS,
            description="Dated snapshot of the ARPAE station registry (raw layer).",
            group_name="ingestion",
            kinds={"rust", "parquet"},
        ),
        AssetSpec(
            RAW_STATION_TYPES,
            description="Dated snapshot of the station types from the ARPAE registry (raw layer).",
            group_name="ingestion",
            kinds={"rust", "parquet"},
        ),
    ],
    partitions_def=daily_partitions,
    # A range of days is one `aq-ingest` call: the ARPAE API costs ~40 s per request
    # whatever the window, so one run per day would be far slower.
    backfill_policy=BackfillPolicy.single_run(),
    retry_policy=INGEST_RETRY_POLICY,
)
def arpae_raw(
    context: AssetExecutionContext, aq_ingest: AqIngestResource
) -> Iterator[MaterializeResult[Any]]:
    """Runs `aq-ingest` for the selected days. The upsert makes reruns idempotent."""
    first_day, last_day = _ingest_window(context)

    context.log.info("Running aq-ingest for %s..%s", first_day, last_day)
    summary = aq_ingest.run(first_day, last_day)
    context.log.info("aq-ingest log:\n%s", summary.log)

    counters = summary.counters
    yield MaterializeResult(
        asset_key=RAW_MEASUREMENTS,
        metadata={
            "first_day": first_day.isoformat(),
            "last_day": last_day.isoformat(),
            **{name: value for name, value in counters.items() if not name.startswith("station_")},
        },
    )
    yield MaterializeResult(
        asset_key=RAW_STATIONS,
        metadata={"rows": counters["station_rows"]} if "station_rows" in counters else {},
    )
    yield MaterializeResult(
        asset_key=RAW_STATION_TYPES,
        metadata=(
            {"rows": counters["station_type_rows"]} if "station_type_rows" in counters else {}
        ),
    )


@asset(
    key=RAW_ARCHIVE,
    description="Validated ARPAE archive files loaded as partitioned Parquet (raw layer).",
    group_name="ingestion",
    kinds={"rust", "parquet"},
)
def arpae_raw_archive(
    context: AssetExecutionContext, aq_ingest: AqIngestResource
) -> MaterializeResult[Any]:
    """Runs `aq-ingest archive` on every archive file found locally.

    Not partitioned: the archive is a fixed set of yearly files, and the upsert
    rewrites nothing when they have not changed.
    """
    context.log.info("Running aq-ingest archive")
    summary = aq_ingest.run(None, None, command="archive")
    context.log.info("aq-ingest log:\n%s", summary.log)
    return MaterializeResult(metadata=dict(summary.counters))


@asset(
    key=RAW_WEATHER,
    description="Hourly Open-Meteo weather at the station coordinates (raw layer).",
    group_name="ingestion",
    kinds={"rust", "parquet"},
    partitions_def=daily_partitions,
    backfill_policy=BackfillPolicy.single_run(),
    retry_policy=INGEST_RETRY_POLICY,
)
def openmeteo_raw_weather(
    context: AssetExecutionContext, aq_ingest: AqIngestResource
) -> MaterializeResult[Any]:
    """Runs `aq-ingest weather` for the selected days."""
    first_day, last_day = _ingest_window(context)

    context.log.info("Running aq-ingest weather for %s..%s", first_day, last_day)
    summary = aq_ingest.run(first_day, last_day, command="weather")
    context.log.info("aq-ingest log:\n%s", summary.log)

    return MaterializeResult(
        metadata={
            "first_day": first_day.isoformat(),
            "last_day": last_day.isoformat(),
            **summary.counters,
        },
    )


@dbt_assets(
    manifest=dbt_project.manifest_path,
    project=dbt_project,
    partitions_def=daily_partitions,
    backfill_policy=BackfillPolicy.single_run(),
)
def aria_er_dbt_assets(context: AssetExecutionContext, dbt: DbtCliResource) -> Iterator[Any]:
    """`dbt build` of the whole project, reprocessing from the first selected day."""
    SETTINGS.duckdb_path.parent.mkdir(parents=True, exist_ok=True)
    reprocess_from = context.partition_time_window.start.date().isoformat()
    dbt_vars = {"reprocess_from": reprocess_from}
    yield from dbt.cli(["build", "--vars", json.dumps(dbt_vars)], context=context).stream()
