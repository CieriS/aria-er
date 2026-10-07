from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

from dagster import (
    AssetSelection,
    DefaultScheduleStatus,
    DefaultSensorStatus,
    RunRequest,
    ScheduleEvaluationContext,
    SensorEvaluationContext,
    SkipReason,
    define_asset_job,
    schedule,
    sensor,
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


def provisional_window_tags(today: date) -> dict[str, str]:
    """Run tags selecting the days in which ARPAE may still revise data, ending today."""
    first_day = today - timedelta(days=SETTINGS.reprocess_window_days() - 1)
    return {
        PARTITION_RANGE_START_TAG: max(first_day.isoformat(), SETTINGS.partitions_start),
        PARTITION_RANGE_END_TAG: today.isoformat(),
    }


def source_today(moment: datetime) -> date:
    """The calendar day of `moment` in ARPAE local standard time."""
    return moment.astimezone(ZoneInfo(SOURCE_TIMEZONE)).date()


@schedule(
    job=refresh_job,
    cron_schedule="0 6 * * *",
    execution_timezone="Europe/Rome",
    default_status=DefaultScheduleStatus.RUNNING,
    description="Every morning, reprocess the window in which ARPAE may still revise data.",
)
def reprocess_provisional_window(context: ScheduleEvaluationContext) -> RunRequest:
    today = source_today(context.scheduled_execution_time)
    return RunRequest(run_key=today.isoformat(), tags=provisional_window_tags(today))


def bootstrap_requests(raw_dir: Path, duckdb_path: Path, today: date) -> list[RunRequest]:
    """Runs needed to fill an empty installation, one step at a time.

    First the archive (the dbt build reads it), then the provisional window with the
    dbt build. Nothing once the warehouse exists. Run keys make each step fire once a day.
    """
    if duckdb_path.exists():
        return []
    if not any(raw_dir.glob("arpae/measurements_archive/*/*/*.parquet")):
        return [RunRequest(job_name=archive_job.name, run_key=f"bootstrap-archive-{today}")]
    return [
        RunRequest(
            job_name=refresh_job.name,
            run_key=f"bootstrap-refresh-{today}",
            tags=provisional_window_tags(today),
        )
    ]


@sensor(
    jobs=[archive_job, refresh_job],
    minimum_interval_seconds=60,
    default_status=DefaultSensorStatus.RUNNING,
    description="On an empty installation, loads the archive and then the latest data.",
)
def bootstrap_empty_warehouse(
    context: SensorEvaluationContext,
) -> list[RunRequest] | SkipReason:
    requests = bootstrap_requests(
        SETTINGS.raw_dir, SETTINGS.duckdb_path, source_today(datetime.now(UTC))
    )
    return requests or SkipReason("The warehouse already exists.")
