"""Chart builders: a mart DataFrame in, an Altair chart out. No Streamlit here."""

from typing import cast

import altair as alt
import pandas as pd

type AnyChart = alt.Chart | alt.LayerChart | alt.FacetChart

LIMIT_COLOR = "#d62728"
SEASON_ORDER = ["winter", "spring", "summer", "autumn"]


def exceedances_chart(frame: pd.DataFrame) -> AnyChart:
    """Exceedances per year and station, with the legal maximum as a red line."""
    bars = (
        alt.Chart(frame)
        .mark_bar()
        .encode(
            x=alt.X("year:O", title="Year"),
            xOffset="station_name:N",
            y=alt.Y("exceedances:Q", title="Exceedances"),
            color=alt.Color("station_name:N", title="Station"),
            tooltip=[
                "station_name",
                "year",
                "exceedances",
                "max_exceedances_per_year",
                alt.Tooltip("year_coverage:Q", format=".0%", title="Year coverage"),
                alt.Tooltip("max_value_ugm3:Q", format=".0f", title="Max value (µg/m³)"),
            ],
        )
    )
    limit = (
        alt.Chart(frame)
        .mark_rule(color=LIMIT_COLOR, strokeDash=[6, 4], strokeWidth=2)
        .encode(y="max(max_exceedances_per_year):Q")
    )
    return alt.layer(bars, limit).properties(height=380)


def trend_chart(frame: pd.DataFrame) -> AnyChart:
    """Annual mean per municipality; hollow points mark non-representative years."""
    base = alt.Chart(frame).encode(
        x=alt.X("year:O", title="Year"),
        y=alt.Y("annual_mean_ugm3:Q", title="Annual mean (µg/m³)"),
        color=alt.Color("municipality:N", title="Municipality"),
    )
    band = base.mark_area(opacity=0.15).encode(
        y="min_station_mean_ugm3:Q", y2="max_station_mean_ugm3:Q"
    )
    points = base.mark_point(size=70).encode(
        fill=alt.condition("datum.is_representative", "municipality:N", alt.value("white")),
        tooltip=[
            "municipality",
            "year",
            "stations",
            alt.Tooltip("annual_mean_ugm3:Q", format=".1f", title="Annual mean (µg/m³)"),
            alt.Tooltip("min_year_coverage:Q", format=".0%", title="Lowest station coverage"),
        ],
    )
    return alt.layer(band, base.mark_line(), points).properties(height=380)


def traffic_vs_background_chart(frame: pd.DataFrame) -> AnyChart:
    """Monthly traffic and background means as two lines."""
    long = frame.melt(
        id_vars=["month", "days"],
        value_vars=["traffic_mean_ugm3", "background_mean_ugm3"],
        var_name="station_type",
        value_name="mean_ugm3",
    )
    long["station_type"] = long["station_type"].map(
        {"traffic_mean_ugm3": "Traffic", "background_mean_ugm3": "Background"}
    )
    return cast(
        alt.Chart,
        alt.Chart(long)
        .mark_line(point=True)
        .encode(
            x=alt.X("yearmonth(month):T", title="Month"),
            y=alt.Y("mean_ugm3:Q", title="Monthly mean (µg/m³)"),
            color=alt.Color("station_type:N", title="Station type"),
            tooltip=[
                alt.Tooltip("yearmonth(month):T", title="Month"),
                "station_type",
                alt.Tooltip("mean_ugm3:Q", format=".1f", title="Mean (µg/m³)"),
                "days",
            ],
        )
        .properties(height=380),
    )


def weather_correlation_chart(frame: pd.DataFrame, column: str, title: str) -> AnyChart:
    """Correlation of PM10 with a weather variable, per station and season."""
    bars = (
        alt.Chart(frame)
        .mark_bar()
        .encode(
            x=alt.X("season:N", sort=SEASON_ORDER, title="Season"),
            xOffset="station_name:N",
            y=alt.Y(f"{column}:Q", title=title, scale=alt.Scale(domain=[-1, 1])),
            color=alt.Color("station_name:N", title="Station"),
            tooltip=[
                "station_name",
                "season",
                "days",
                alt.Tooltip(f"{column}:Q", format=".2f", title=title),
            ],
        )
    )
    zero = alt.Chart(pd.DataFrame({"y": [0]})).mark_rule(color="gray").encode(y="y:Q")
    return alt.layer(bars, zero).properties(height=320)


def completeness_chart(frame: pd.DataFrame) -> AnyChart:
    """Daily completeness per pollutant as a heatmap; empty days stand out in red."""
    return cast(
        alt.Chart,
        alt.Chart(frame)
        .mark_rect()
        .encode(
            x=alt.X("yearmonthdate(measurement_date):T", title="Day"),
            y=alt.Y("pollutant_code:N", title="Pollutant"),
            color=alt.Color(
                "completeness:Q",
                title="Completeness",
                scale=alt.Scale(domain=[0, 1], scheme="redyellowgreen"),
                legend=alt.Legend(format=".0%"),
            ),
            tooltip=[
                alt.Tooltip("measurement_date:T", title="Day"),
                "pollutant_code",
                "valid_measurements",
                "expected_measurements",
                alt.Tooltip("completeness:Q", format=".0%"),
            ],
        )
        .properties(height=alt.Step(28)),
    )
