"""Every page renders without errors on a small warehouse, and on a missing one."""

from pathlib import Path

import pytest
from streamlit.testing.v1 import AppTest

from aria_er_dashboard import charts, views
from aria_er_dashboard.data import Marts

APP = str(Path(__file__).resolve().parents[1] / "src/aria_er_dashboard/app.py")
PAGES = ["exceedances", "trend", "traffic_vs_background", "weather", "completeness"]


def _page(name: str) -> None:
    """Script run by AppTest: only its body is executed, so it imports what it needs."""
    from aria_er_dashboard import views

    getattr(views, name)()


def run_page(name: str) -> AppTest:
    return AppTest.from_function(_page, args=(name,)).run()


@pytest.mark.parametrize("name", PAGES)
def test_page_renders_with_a_chart(
    name: str, warehouse_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assert callable(getattr(views, name))
    monkeypatch.setenv("AQ_DUCKDB_PATH", str(warehouse_path))

    app = run_page(name)

    assert not app.exception, app.exception
    assert len(app.title) == 1
    # A caption states the question the page answers.
    assert app.caption[0].value.endswith("?")
    assert not app.info, "the default selection should show data"


def test_app_reports_a_missing_warehouse(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("AQ_DUCKDB_PATH", str(tmp_path / "missing.duckdb"))

    app = AppTest.from_file(APP).run()

    assert not app.exception
    assert "make transform" in app.error[0].value


def test_emptying_a_filter_shows_a_message_instead_of_a_chart(
    warehouse_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("AQ_DUCKDB_PATH", str(warehouse_path))
    app = run_page("weather")

    app.sidebar.multiselect[0].set_value([]).run()

    assert not app.exception
    assert app.info[0].value == "No data for this selection."


def test_charts_build_from_mart_rows(marts: Marts) -> None:
    built = [
        charts.exceedances_chart(marts.exceedances("daily_mean")),
        charts.trend_chart(marts.pollutant_trend("PM10")),
        charts.traffic_vs_background_chart(marts.traffic_vs_background("NO2", "BOLOGNA")),
        charts.weather_correlation_chart(marts.weather_correlation(), "corr_pm10_wind", "r"),
        charts.completeness_chart(marts.completeness("GIARDINI MARGHERITA")),
    ]
    for chart in built:
        # Serialising validates the specification against the Vega-Lite schema.
        assert chart.to_dict()["$schema"].startswith("https://vega.github.io/schema/vega-lite")
