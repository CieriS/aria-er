"""Pages of the dashboard: widgets, a mart query, a chart. No calculations."""

from datetime import timedelta

import streamlit as st

from aria_er_dashboard import charts
from aria_er_dashboard.data import Marts, duckdb_path_from_env

METRIC_LABELS = {
    "daily_mean": "PM10 — days with a daily mean above the limit",
    "hourly_mean": "NO2 — hours with an hourly mean above the limit",
    "daily_max_8h_mean": "O3 — days with an 8-hour maximum above the target",
}


def _marts() -> Marts:
    return Marts(duckdb_path_from_env())


def _strings(values: list[object]) -> list[str]:
    return [str(value) for value in values]


# Stations with the longest history loaded, shown first when present.
PREFERRED_STATIONS = ("GIARDINI MARGHERITA", "PORTA SAN FELICE", "VIA CHIARINI")
# Days shown by default on the completeness page.
DEFAULT_COMPLETENESS_DAYS = 365


def _default_stations(names: list[str]) -> list[str]:
    return [name for name in PREFERRED_STATIONS if name in names] or names[:3]


def _year_filter(marts: Marts, mart: str) -> tuple[int, int] | None:
    years = [int(str(year)) for year in marts.distinct(mart, "year")]
    if len(years) < 2:
        return None
    first, last = st.sidebar.slider("Years", min(years), max(years), (min(years), max(years)))
    return int(first), int(last)


def exceedances() -> None:
    st.title("Exceedances of legal limits")
    st.caption("How many times did each station exceed the legal limits, per year?")
    marts = _marts()

    metrics = [m for m in METRIC_LABELS if m in marts.distinct("mart_exceedances_yearly", "metric")]
    metric = st.sidebar.selectbox(
        "Pollutant and limit", metrics, format_func=lambda name: METRIC_LABELS[name]
    )
    if metric is None:
        st.info("No data for this selection.")
        return
    names = _strings(marts.distinct("mart_exceedances_yearly", "station_name"))
    stations = st.sidebar.multiselect("Stations", names, default=_default_stations(names))
    years = _year_filter(marts, "mart_exceedances_yearly")

    frame = marts.exceedances(metric, stations, years)
    if frame.empty:
        st.info("No data for this selection.")
        return
    allowed = int(frame["max_exceedances_per_year"].iloc[0])
    limit = float(frame["limit_ugm3"].iloc[0])
    st.markdown(
        f"Limit: **{limit:.0f} µg/m³**, at most **{allowed}** exceedances per year "
        "(red dashed line)."
    )
    st.altair_chart(charts.exceedances_chart(frame), width="stretch")
    st.caption(
        "A year with low coverage undercounts: check the coverage in the tooltip or the table."
    )
    st.dataframe(frame, hide_index=True)


def trend() -> None:
    st.title("Multi-year trend")
    st.caption("Is air quality improving over the years, by pollutant and city?")
    marts = _marts()

    pollutants = _strings(marts.distinct("mart_pollutant_trend", "pollutant_code"))
    index = pollutants.index("PM10") if "PM10" in pollutants else 0
    pollutant = st.sidebar.selectbox("Pollutant", pollutants, index=index)
    names = _strings(marts.distinct("mart_pollutant_trend", "municipality"))
    default = ["BOLOGNA"] if "BOLOGNA" in names else names[:1]
    municipalities = st.sidebar.multiselect("Municipalities", names, default=default)
    years = _year_filter(marts, "mart_pollutant_trend")

    frame = marts.pollutant_trend(pollutant, municipalities, years)
    if frame.empty:
        st.info("No data for this selection.")
        return
    st.altair_chart(charts.trend_chart(frame), width="stretch")
    st.caption(
        "Line: average of the stations' annual means. Band: lowest to highest station. "
        "Hollow points: years in which a station has less than 90% of valid days."
    )
    st.dataframe(frame, hide_index=True)


def traffic_vs_background() -> None:
    st.title("Traffic vs background")
    st.caption("How much does traffic add to the background pollution of the same city?")
    marts = _marts()

    names = _strings(marts.distinct("mart_traffic_vs_background", "municipality"))
    if not names:
        st.info("No municipality has both a traffic and a background station classified.")
        return
    municipality = st.sidebar.selectbox("Municipality", names)
    pollutants = _strings(marts.distinct("mart_traffic_vs_background", "pollutant_code"))
    index = pollutants.index("NO2") if "NO2" in pollutants else 0
    pollutant = st.sidebar.selectbox("Pollutant", pollutants, index=index)
    bounds = marts.date_range("mart_traffic_vs_background", "month")
    period = None
    if bounds is not None and bounds[0] < bounds[1]:
        selected = st.sidebar.slider("Period", bounds[0], bounds[1], bounds, format="MMM YYYY")
        period = (selected[0], selected[1])

    frame = marts.traffic_vs_background(pollutant, municipality, period)
    if frame.empty:
        st.info("No data for this selection.")
        return
    st.altair_chart(charts.traffic_vs_background_chart(frame), width="stretch")
    st.caption("Monthly means over the days on which both kinds of station have valid data.")
    st.dataframe(frame, hide_index=True)


def weather() -> None:
    st.title("Weather and PM10")
    st.caption("How much of a PM10 peak is explained by wind and rain?")
    marts = _marts()

    names = _strings(marts.distinct("mart_weather_correlation", "station_name"))
    stations = st.sidebar.multiselect("Stations", names, default=_default_stations(names))
    seasons = st.sidebar.multiselect("Seasons", charts.SEASON_ORDER, default=charts.SEASON_ORDER)

    frame = marts.weather_correlation(stations, seasons)
    if frame.empty:
        st.info("No data for this selection.")
        return
    st.altair_chart(
        charts.weather_correlation_chart(frame, "corr_pm10_wind", "Correlation PM10 vs wind"),
        width="stretch",
    )
    st.altair_chart(
        charts.weather_correlation_chart(
            frame, "corr_pm10_precipitation", "Correlation PM10 vs precipitation"
        ),
        width="stretch",
    )
    st.caption(
        "Pearson correlation of daily values. Negative: more wind or rain goes with less PM10. "
        "Weather is a ~10 km reanalysis, not a measurement at the station."
    )
    st.dataframe(frame, hide_index=True)


def completeness() -> None:
    st.title("Data completeness")
    st.caption("How complete are the data of each station, day by day?")
    marts = _marts()

    names = _strings(marts.distinct("mart_data_completeness", "station_name"))
    index = names.index("GIARDINI MARGHERITA") if "GIARDINI MARGHERITA" in names else 0
    station = st.sidebar.selectbox("Station", names, index=index)
    pollutants = _strings(marts.distinct("mart_data_completeness", "pollutant_code"))
    selected = st.sidebar.multiselect("Pollutants", pollutants, default=pollutants)
    bounds = marts.date_range("mart_data_completeness", "measurement_date")
    period = None
    if bounds is not None and bounds[0] < bounds[1]:
        recent = max(bounds[0], bounds[1] - timedelta(days=DEFAULT_COMPLETENESS_DAYS))
        chosen = st.sidebar.slider("Period", bounds[0], bounds[1], (recent, bounds[1]))
        period = (chosen[0], chosen[1])

    frame = marts.completeness(station, selected, period)
    if frame.empty:
        st.info("No data for this selection.")
        return
    st.altair_chart(charts.completeness_chart(frame), width="stretch")
    st.caption(
        "Share of expected values that are valid, per day. Red days have no usable data; "
        "a day counts as complete from 75%."
    )
