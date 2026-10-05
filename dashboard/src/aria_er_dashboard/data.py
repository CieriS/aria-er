"""Read-only access to the dbt marts.

The dashboard reads marts only and never computes: every function here is a
filtered `select` on one mart. Calculations belong in dbt.
"""

import os
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import date
from pathlib import Path

import duckdb
import pandas as pd

DEFAULT_DUCKDB_PATH = Path(__file__).resolve().parents[3] / "warehouse" / "aria_er.duckdb"


class MartsUnavailableError(RuntimeError):
    """The warehouse file is missing or a mart has not been built."""


def duckdb_path_from_env() -> Path:
    """Warehouse location: `AQ_DUCKDB_PATH`, or the repository default."""
    value = os.environ.get("AQ_DUCKDB_PATH")
    return Path(value) if value else DEFAULT_DUCKDB_PATH


def _in_filter(column: str, values: Sequence[object] | None) -> tuple[str, list[object]]:
    """SQL condition and parameters selecting `values`; no condition when it is None."""
    if values is None:
        return "true", []
    if not values:
        return "false", []
    return f"{column} in ({', '.join('?' for _ in values)})", list(values)


@dataclass(frozen=True)
class Marts:
    """The marts of one DuckDB warehouse."""

    duckdb_path: Path

    def _query(self, sql: str, parameters: Sequence[object] = ()) -> pd.DataFrame:
        if not self.duckdb_path.is_file():
            raise MartsUnavailableError(
                f"Warehouse not found at {self.duckdb_path}. Run `make transform` first."
            )
        try:
            with duckdb.connect(str(self.duckdb_path), read_only=True) as connection:
                return connection.execute(sql, list(parameters)).df()
        except duckdb.CatalogException as error:
            raise MartsUnavailableError(
                f"A mart is missing in {self.duckdb_path}. Run `make transform`. ({error})"
            ) from error

    def distinct(self, mart: str, column: str) -> list[object]:
        """Sorted distinct non-null values of a mart column, for filter widgets."""
        if not (mart.startswith("mart_") and mart.isidentifier() and column.isidentifier()):
            raise ValueError(f"not a mart column: {mart}.{column}")
        frame = self._query(
            f"select distinct {column} as value from {mart} where {column} is not null order by 1"
        )
        return list(frame["value"])

    def exceedances(
        self,
        metric: str,
        stations: Sequence[str] | None = None,
        years: tuple[int, int] | None = None,
    ) -> pd.DataFrame:
        """Rows of `mart_exceedances_yearly` for one metric of one pollutant."""
        station_sql, parameters = _in_filter("station_name", stations)
        year_sql = "true"
        if years is not None:
            year_sql = "year between ? and ?"
            parameters += [years[0], years[1]]
        return self._query(
            f"""
            select station_id, station_name, municipality, pollutant_code, metric, year,
                   exceedances, limit_ugm3, max_exceedances_per_year,
                   is_over_allowed_exceedances, max_value_ugm3, valid_periods, year_coverage
            from mart_exceedances_yearly
            where metric = ? and {station_sql} and {year_sql}
            order by station_name, year
            """,
            [metric, *parameters],
        )

    def pollutant_trend(
        self,
        pollutant: str,
        municipalities: Sequence[str] | None = None,
        years: tuple[int, int] | None = None,
    ) -> pd.DataFrame:
        """Rows of `mart_pollutant_trend` for one pollutant."""
        municipality_sql, parameters = _in_filter("municipality", municipalities)
        year_sql = "true"
        if years is not None:
            year_sql = "year between ? and ?"
            parameters += [years[0], years[1]]
        return self._query(
            f"""
            select municipality, pollutant_code, year, stations, annual_mean_ugm3,
                   min_station_mean_ugm3, max_station_mean_ugm3, min_year_coverage,
                   is_representative
            from mart_pollutant_trend
            where pollutant_code = ? and {municipality_sql} and {year_sql}
            order by municipality, year
            """,
            [pollutant, *parameters],
        )

    def traffic_vs_background(
        self,
        pollutant: str,
        municipality: str,
        period: tuple[date, date] | None = None,
    ) -> pd.DataFrame:
        """Rows of `mart_traffic_vs_background` for one pollutant and municipality."""
        period_sql, parameters = "true", []
        if period is not None:
            period_sql = "month between ? and ?"
            parameters = [period[0], period[1]]
        return self._query(
            f"""
            select municipality, pollutant_code, month, days, traffic_stations,
                   background_stations, traffic_mean_ugm3, background_mean_ugm3,
                   difference_ugm3, traffic_to_background_ratio
            from mart_traffic_vs_background
            where pollutant_code = ? and municipality = ? and {period_sql}
            order by month
            """,
            [pollutant, municipality, *parameters],
        )

    def weather_correlation(
        self,
        stations: Sequence[str] | None = None,
        seasons: Sequence[str] | None = None,
    ) -> pd.DataFrame:
        """Rows of `mart_weather_correlation`."""
        station_sql, station_parameters = _in_filter("station_name", stations)
        season_sql, season_parameters = _in_filter("season", seasons)
        return self._query(
            f"""
            select station_id, station_name, municipality, season, days, pm10_mean_ugm3,
                   corr_pm10_wind, corr_pm10_precipitation, windy_days,
                   pm10_mean_windy_ugm3, pm10_mean_calm_ugm3, rainy_days,
                   pm10_mean_rainy_ugm3, pm10_mean_dry_ugm3
            from mart_weather_correlation
            where {station_sql} and {season_sql}
            order by station_name, season
            """,
            [*station_parameters, *season_parameters],
        )

    def completeness(
        self,
        station: str,
        pollutants: Sequence[str] | None = None,
        period: tuple[date, date] | None = None,
    ) -> pd.DataFrame:
        """Rows of `mart_data_completeness` for one station."""
        pollutant_sql, parameters = _in_filter("pollutant_code", pollutants)
        period_sql = "true"
        if period is not None:
            period_sql = "measurement_date between ? and ?"
            parameters += [period[0], period[1]]
        return self._query(
            f"""
            select station_id, station_name, pollutant_code, measurement_date,
                   expected_measurements, valid_measurements, completeness, is_complete
            from mart_data_completeness
            where station_name = ? and {pollutant_sql} and {period_sql}
            order by pollutant_code, measurement_date
            """,
            [station, *parameters],
        )

    def date_range(self, mart: str, column: str) -> tuple[date, date] | None:
        """First and last value of a date column, or None when the mart is empty."""
        if not (mart.startswith("mart_") and mart.isidentifier() and column.isidentifier()):
            raise ValueError(f"not a mart column: {mart}.{column}")
        frame = self._query(f"select min({column}) as first, max({column}) as last from {mart}")
        first, last = frame["first"][0], frame["last"][0]
        if pd.isna(first) or pd.isna(last):
            return None
        return pd.Timestamp(first).date(), pd.Timestamp(last).date()
