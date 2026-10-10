# Dashboard (Streamlit)

Pages, questions answered and structure of the app.

## Dashboard (Streamlit)

`make dashboard` opens the dashboard on the marts built by `make transform`. Each page
answers one of the questions at the top of this README:

| Page | Question | Mart |
|---|---|---|
| Exceedances | How many times did each station exceed the legal limits, per year? The legal maximum is a red line. | `mart_exceedances_yearly` |
| Trend | Is air quality improving over the years, by pollutant and city? | `mart_pollutant_trend` |
| Traffic vs background | How much does traffic add to the background pollution of the same city? | `mart_traffic_vs_background` |
| Weather and PM10 | How much of a PM10 peak is explained by wind and rain? | `mart_weather_correlation` |
| Data completeness | How complete are the data of each station, day by day? | `mart_data_completeness` |

Filters (stations, pollutant, municipality, period) are in the sidebar of each page.

The dashboard reads marts only and computes nothing: `data.py` holds one filtered `select`
per mart, `charts.py` turns a mart DataFrame into an Altair chart, `views.py` wires widgets
to the two. Thresholds, means and correlations all come from dbt. The warehouse is opened
read-only; its path can be changed with `AQ_DUCKDB_PATH`.
