from dagster import Definitions
from dagster_dbt import DbtCliResource

from aria_er_orchestration.assets import (
    aria_er_dbt_assets,
    arpae_raw,
    arpae_raw_archive,
    dbt_project,
    openmeteo_raw_weather,
)
from aria_er_orchestration.checks import raw_measurements_freshness, recent_data_completeness
from aria_er_orchestration.resources import AqIngestResource, WarehouseResource
from aria_er_orchestration.schedules import (
    archive_job,
    bootstrap_empty_warehouse,
    refresh_job,
    reprocess_provisional_window,
)
from aria_er_orchestration.settings import SETTINGS

defs = Definitions(
    assets=[arpae_raw, arpae_raw_archive, openmeteo_raw_weather, aria_er_dbt_assets],
    asset_checks=[raw_measurements_freshness, recent_data_completeness],
    jobs=[refresh_job, archive_job],
    schedules=[reprocess_provisional_window],
    sensors=[bootstrap_empty_warehouse],
    resources={
        "aq_ingest": AqIngestResource(
            binary_path=str(SETTINGS.ingest_binary),
            config_path=str(SETTINGS.ingest_config),
            working_dir=str(SETTINGS.repo_root),
        ),
        "dbt": DbtCliResource(project_dir=dbt_project),
        "warehouse": WarehouseResource(
            raw_dir=str(SETTINGS.raw_dir),
            duckdb_path=str(SETTINGS.duckdb_path),
        ),
    },
)
