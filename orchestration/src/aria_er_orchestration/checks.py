from datetime import UTC, datetime

from dagster import AssetCheckResult, AssetCheckSeverity, AssetKey, asset_check

from aria_er_orchestration.assets import RAW_MEASUREMENTS
from aria_er_orchestration.resources import WarehouseResource

MAX_AGE_HOURS = 48
COMPLETENESS_WINDOW_DAYS = 30
MIN_COMPLETE_SHARE = 0.9

COMPLETENESS_MART = AssetKey(["mart_data_completeness"])


@asset_check(
    asset=RAW_MEASUREMENTS,
    description=f"The newest measurement in the raw layer is at most {MAX_AGE_HOURS} hours old.",
)
def raw_measurements_freshness(warehouse: WarehouseResource) -> AssetCheckResult:
    latest = warehouse.latest_measurement()
    if latest is None:
        return AssetCheckResult(
            passed=False,
            severity=AssetCheckSeverity.WARN,
            description="The raw layer has no measurements.",
        )
    age_hours = (datetime.now(UTC) - latest.astimezone(UTC)).total_seconds() / 3600
    return AssetCheckResult(
        passed=age_hours <= MAX_AGE_HOURS,
        severity=AssetCheckSeverity.WARN,
        description=f"Newest measurement is {age_hours:.0f} hours old.",
        metadata={"latest_measurement_utc": latest.isoformat(), "age_hours": round(age_hours, 1)},
    )


@asset_check(
    asset=COMPLETENESS_MART,
    description=(
        f"At least {MIN_COMPLETE_SHARE:.0%} of station-pollutant-days are complete "
        f"over the last {COMPLETENESS_WINDOW_DAYS} days of data."
    ),
)
def recent_data_completeness(warehouse: WarehouseResource) -> AssetCheckResult:
    complete, total = warehouse.completeness(COMPLETENESS_WINDOW_DAYS)
    share = complete / total if total else 0.0
    return AssetCheckResult(
        passed=share >= MIN_COMPLETE_SHARE,
        severity=AssetCheckSeverity.WARN,
        description=f"{complete} of {total} station-pollutant-days complete ({share:.1%}).",
        metadata={"complete_days": complete, "total_days": total, "complete_share": share},
    )
