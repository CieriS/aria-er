"""Entry point: `streamlit run src/aria_er_dashboard/app.py`."""

import streamlit as st

from aria_er_dashboard import views
from aria_er_dashboard.data import MartsUnavailableError

PAGES = [
    st.Page(views.exceedances, title="Exceedances", url_path="exceedances", default=True),
    st.Page(views.trend, title="Trend", url_path="trend"),
    st.Page(views.traffic_vs_background, title="Traffic vs background", url_path="traffic"),
    st.Page(views.weather, title="Weather and PM10", url_path="weather"),
    st.Page(views.completeness, title="Data completeness", url_path="completeness"),
]


def main() -> None:
    st.set_page_config(page_title="aria-er — air quality in Emilia-Romagna", layout="wide")
    page = st.navigation(PAGES)
    try:
        page.run()
    except MartsUnavailableError as error:
        st.error(str(error))


main()
