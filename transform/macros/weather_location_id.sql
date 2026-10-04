{#- Weather location of a point: its coordinates scaled and rounded, as built by the
    ingestor (`<lat>_<lon>`, e.g. 445_114 for 44.5 N 11.4 E at one decimal). -#}
{% macro weather_location_id(latitude, longitude) -%}
    {%- set scale = 10 ** var('weather_coordinate_decimals') -%}
    cast(cast(round({{ latitude }} * {{ scale }}) as bigint) as varchar)
        || '_'
        || cast(cast(round({{ longitude }} * {{ scale }}) as bigint) as varchar)
{%- endmacro %}
