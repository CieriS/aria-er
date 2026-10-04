from datetime import datetime
from zoneinfo import ZoneInfo

from dagster import AssetKey, Definitions, RunRequest, build_schedule_context
from dagster._core.definitions.unresolved_asset_job_definition import (
    UnresolvedAssetJobDefinition,
)

from aria_er_orchestration.assets import (
    RAW_MEASUREMENTS,
    RAW_STATIONS,
    RAW_WEATHER,
    arpae_raw,
    openmeteo_raw_weather,
)
from aria_er_orchestration.definitions import defs
from aria_er_orchestration.partitions import daily_partitions
from aria_er_orchestration.resources import AqIngestResource, WarehouseResource
from aria_er_orchestration.schedules import (
    PARTITION_RANGE_END_TAG,
    PARTITION_RANGE_START_TAG,
    reprocess_provisional_window,
)
from aria_er_orchestration.settings import SETTINGS


def test_definitions_load() -> None:
    Definitions.validate_loadable(defs)


def test_lineage_runs_from_ingestion_to_marts() -> None:
    graph = defs.resolve_asset_graph()

    staging = AssetKey(["stg_arpae__measurements"])
    assert graph.get(staging).parent_keys == {RAW_MEASUREMENTS}
    assert RAW_STATIONS in graph.get(AssetKey(["stg_arpae__stations"])).parent_keys

    for mart in ("mart_exceedances_yearly", "mart_data_completeness"):
        ancestors = graph.get_ancestor_asset_keys(AssetKey([mart]))
        assert {RAW_MEASUREMENTS, RAW_STATIONS} <= ancestors, mart


def test_weather_is_upstream_of_the_correlation_mart() -> None:
    graph = defs.resolve_asset_graph()

    assert graph.get(AssetKey(["stg_openmeteo__weather"])).parent_keys == {RAW_WEATHER}
    ancestors = graph.get_ancestor_asset_keys(AssetKey(["mart_weather_correlation"]))
    assert {RAW_WEATHER, RAW_MEASUREMENTS, RAW_STATIONS} <= ancestors


def test_every_materializable_asset_is_daily_partitioned() -> None:
    graph = defs.resolve_asset_graph()
    for key in graph.materializable_asset_keys:
        assert graph.get(key).partitions_def == daily_partitions, key


def test_ingestion_has_a_retry_policy() -> None:
    for ingestion in (arpae_raw, openmeteo_raw_weather):
        policy = ingestion.op.retry_policy
        assert policy is not None
        assert policy.max_retries == 2


def test_checks_cover_freshness_and_completeness() -> None:
    names = {key.name for key in defs.resolve_asset_graph().asset_check_keys}
    assert {"raw_measurements_freshness", "recent_data_completeness"} <= names
    # dbt tests are loaded as asset checks too.
    assert len(names) > 2


def test_resources_point_at_the_repository_components() -> None:
    resources = defs.resources
    assert resources is not None

    ingest = resources["aq_ingest"]
    assert isinstance(ingest, AqIngestResource)
    assert ingest.binary_path.endswith("aq-ingest")
    assert ingest.config_path == str(SETTINGS.ingest_config)
    assert SETTINGS.ingest_config.is_file()

    warehouse = resources["warehouse"]
    assert isinstance(warehouse, WarehouseResource)
    assert warehouse.duckdb_path.endswith(".duckdb")
    assert "dbt" in resources


def test_schedule_requests_the_whole_provisional_window_in_one_run() -> None:
    scheduled = datetime(2026, 10, 2, 6, 0, tzinfo=ZoneInfo("Europe/Rome"))
    request = reprocess_provisional_window(
        build_schedule_context(scheduled_execution_time=scheduled)
    )

    assert isinstance(request, RunRequest)
    assert request.tags[PARTITION_RANGE_END_TAG] == "2026-10-02"
    # 30 days inclusive, as configured for the ingestor.
    assert SETTINGS.reprocess_window_days() == 30
    assert request.tags[PARTITION_RANGE_START_TAG] == "2026-09-03"


def test_schedule_never_starts_before_the_first_partition() -> None:
    scheduled = datetime(2026, 8, 5, 6, 0, tzinfo=ZoneInfo("Europe/Rome"))
    request = reprocess_provisional_window(
        build_schedule_context(scheduled_execution_time=scheduled)
    )

    assert isinstance(request, RunRequest)
    assert request.tags[PARTITION_RANGE_START_TAG] == daily_partitions.get_first_partition_key()


def test_schedule_is_on_by_default_and_targets_the_refresh_job() -> None:
    schedule = defs.resolve_schedule_def("reprocess_provisional_window")
    assert schedule.default_status.value == "RUNNING"
    assert schedule.job_name == "arpae_refresh"
    assert isinstance(schedule.job, UnresolvedAssetJobDefinition)
