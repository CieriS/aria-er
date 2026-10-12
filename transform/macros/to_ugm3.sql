{#-
    Converts a concentration to µg/m³. Units that are not a mass concentration give null.
    The yearly archive files write `ug/m3`, the REST measurement archive `µg/m³`.
-#}
{% macro to_ugm3(value, unit) -%}
    case
        when {{ unit }} in ('ug/m3', 'µg/m³') then {{ value }}
        when {{ unit }} in ('mg/m3', 'mg/m³') then {{ value }} * 1000
    end
{%- endmacro %}
