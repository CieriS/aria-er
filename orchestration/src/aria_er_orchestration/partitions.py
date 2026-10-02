from dagster import DailyPartitionsDefinition

from aria_er_orchestration.settings import SETTINGS

# ARPAE days are in local standard time (UTC+1 all year). "Etc/GMT-1" is UTC+1.
SOURCE_TIMEZONE = "Etc/GMT-1"

# One partition per day of measurements. `end_offset=1` includes the current day,
# which already has provisional data.
daily_partitions = DailyPartitionsDefinition(
    start_date=SETTINGS.partitions_start,
    timezone=SOURCE_TIMEZONE,
    end_offset=1,
)
