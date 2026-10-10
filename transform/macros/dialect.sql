{#-
  SQL that differs between DuckDB and BigQuery, in one place. Everything else in the
  project is written in the subset both understand (case expressions instead of FILTER,
  extract() instead of year(), dbt's own datediff and date_trunc).

  A "naive UTC timestamp" is a wall-clock time in UTC without zone: TIMESTAMP in DuckDB,
  DATETIME in BigQuery (whose TIMESTAMP is always an absolute instant).
-#}

{# Type of a naive UTC timestamp. #}
{% macro naive_timestamp_type() -%}
    {{ return(adapter.dispatch('naive_timestamp_type', 'aria_er')()) }}
{%- endmacro %}

{% macro default__naive_timestamp_type() -%} timestamp {%- endmacro %}
{% macro bigquery__naive_timestamp_type() -%} datetime {%- endmacro %}


{# An absolute instant (as written by the ingestor) as a naive UTC timestamp. #}
{% macro to_naive_utc(column) -%}
    {{ return(adapter.dispatch('to_naive_utc', 'aria_er')(column)) }}
{%- endmacro %}

{% macro default__to_naive_utc(column) -%}
    cast({{ column }} at time zone 'UTC' as timestamp)
{%- endmacro %}

{% macro bigquery__to_naive_utc(column) -%}
    datetime({{ column }}, 'UTC')
{%- endmacro %}


{# A date from year, month and day expressions. #}
{% macro make_date(year, month, day) -%}
    {{ return(adapter.dispatch('make_date', 'aria_er')(year, month, day)) }}
{%- endmacro %}

{% macro default__make_date(year, month, day) -%}
    make_date({{ year }}, {{ month }}, {{ day }})
{%- endmacro %}

{% macro bigquery__make_date(year, month, day) -%}
    date({{ year }}, {{ month }}, {{ day }})
{%- endmacro %}


{# Number of days of a calendar year. #}
{% macro days_in_year(year) -%}
    {{ dbt.datediff(make_date(year, 1, 1), make_date(year ~ ' + 1', 1, 1), 'day') }}
{%- endmacro %}


{# Whole hours since 1970 of a naive timestamp: a numeric key for RANGE window frames,
   which BigQuery only accepts on numbers. #}
{% macro hours_since_epoch(column) -%}
    {{ return(adapter.dispatch('hours_since_epoch', 'aria_er')(column)) }}
{%- endmacro %}

{% macro default__hours_since_epoch(column) -%}
    cast(epoch({{ column }}) / 3600 as bigint)
{%- endmacro %}

{% macro bigquery__hours_since_epoch(column) -%}
    datetime_diff({{ column }}, datetime '1970-01-01', hour)
{%- endmacro %}


{# A FROM item producing one row per day between two dates (inclusive), as a column
   called `spine_date`. Use it after a comma or CROSS JOIN, correlated to the dates. #}
{% macro unnest_days(first_date, last_date) -%}
    {{ return(adapter.dispatch('unnest_days', 'aria_er')(first_date, last_date)) }}
{%- endmacro %}

{% macro default__unnest_days(first_date, last_date) -%}
    unnest(generate_series({{ first_date }}, {{ last_date }}, interval 1 day)) as spine (spine_date)
{%- endmacro %}

{% macro bigquery__unnest_days(first_date, last_date) -%}
    unnest(generate_date_array({{ first_date }}, {{ last_date }})) as spine_date
{%- endmacro %}


{# Incremental strategy that replaces rows by key. #}
{% macro replace_by_key_strategy() -%}
    {{ return('merge' if target.type == 'bigquery' else 'delete+insert') }}
{%- endmacro %}
