from datetime import timedelta
from zoneinfo import ZoneInfo

from dagster import (
    AssetSelection,
    DefaultScheduleStatus,
    RunRequest,
    ScheduleEvaluationContext,
    define_asset_job,
    schedule,
)

from aria_er_orchestration.assets import RAW_ARCHIVE
from aria_er_orchestration.partitions import SOURCE_TIMEZONE
from aria_er_orchestration.settings import SETTINGS

# Dagster tags that make one run cover a range of partitions.
PARTITION_RANGE_START_TAG = "dagster/asset_partition_range_start"
PARTITION_RANGE_END_TAG = "dagster/asset_partition_range_end"

# The archive is a fixed set of files, not a daily partition: it has its own job.
ARCHIVE_SELECTION = AssetSelection.assets(RAW_ARCHIVE)

refresh_job = define_asset_job(
    "arpae_refresh",
    selection=AssetSelection.all() - ARCHIVE_SELECTION,
    description="Ingestion followed by the dbt build, for a day or a range of days.",
)


archive_job = define_asset_job(
    "arpae_archive_load",
    selection=ARCHIVE_SELECTION,
    description="Loads the local ARPAE archive files into the raw layer.",
)


@schedule(
    job=refresh_job,
    cron_schedule="0 6 * * *",
    execution_timezone="Europe/Rome",
    default_status=DefaultScheduleStatus.RUNNING,
    description="Every morning, reprocess the window in which ARPAE may still revise data.",
)
def reprocess_provisional_window(context: ScheduleEvaluationContext) -> RunRequest:
    today = context.scheduled_execution_time.astimezone(ZoneInfo(SOURCE_TIMEZONE)).date()
    first_day = today - timedelta(days=SETTINGS.reprocess_window_days() - 1)
    start = max(first_day.isoformat(), SETTINGS.partitions_start)
    return RunRequest(
        run_key=today.isoformat(),
        tags={
            PARTITION_RANGE_START_TAG: start,
            PARTITION_RANGE_END_TAG: today.isoformat(),
        },
    )
