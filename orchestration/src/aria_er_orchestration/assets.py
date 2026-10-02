import json
from collections.abc import Iterator
from datetime import timedelta
from typing import Any

from dagster import (
    AssetExecutionContext,
    AssetKey,
    AssetSpec,
    BackfillPolicy,
    Backoff,
    MaterializeResult,
    RetryPolicy,
    multi_asset,
)
from dagster_dbt import DbtCliResource, DbtProject, dbt_assets

from aria_er_orchestration.partitions import daily_partitions
from aria_er_orchestration.resources import AqIngestResource
from aria_er_orchestration.settings import SETTINGS

# Same keys dagster-dbt gives to the dbt sources, so the lineage connects.
RAW_MEASUREMENTS = AssetKey(["arpae_raw", "measurements"])
RAW_STATIONS = AssetKey(["arpae_raw", "stations"])

dbt_project = DbtProject(
    project_dir=SETTINGS.dbt_project_dir,
    profiles_dir=SETTINGS.dbt_project_dir,
)
# `dagster dev` regenerates the manifest on load; elsewhere build it only if missing.
dbt_project.prepare_if_dev()
if not dbt_project.manifest_path.exists():
    dbt_project.preparer.prepare(dbt_project)


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
    ],
    partitions_def=daily_partitions,
    # A range of days is one `aq-ingest` call: the ARPAE API costs ~40 s per request
    # whatever the window, so one run per day would be far slower.
    backfill_policy=BackfillPolicy.single_run(),
    # The ARPAE API fails intermittently beyond what the ingestor's own retries cover.
    retry_policy=RetryPolicy(max_retries=2, delay=60, backoff=Backoff.EXPONENTIAL),
)
def arpae_raw(
    context: AssetExecutionContext, aq_ingest: AqIngestResource
) -> Iterator[MaterializeResult[Any]]:
    """Runs `aq-ingest` for the selected days. The upsert makes reruns idempotent."""
    window = context.partition_time_window
    first_day = window.start.date()
    last_day = (window.end - timedelta(days=1)).date()

    context.log.info("Running aq-ingest for %s..%s", first_day, last_day)
    summary = aq_ingest.run(first_day, last_day)
    context.log.info("aq-ingest log:\n%s", summary.log)

    counters = summary.counters
    yield MaterializeResult(
        asset_key=RAW_MEASUREMENTS,
        metadata={
            "first_day": first_day.isoformat(),
            "last_day": last_day.isoformat(),
            **{name: counters[name] for name in counters if name != "station_rows"},
        },
    )
    yield MaterializeResult(
        asset_key=RAW_STATIONS,
        metadata={"rows": counters["station_rows"]} if "station_rows" in counters else {},
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
