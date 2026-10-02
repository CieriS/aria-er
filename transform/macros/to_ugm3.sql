{#- Converts a concentration to µg/m³. Units that are not a mass concentration give null. -#}
{% macro to_ugm3(value, unit) -%}
    case {{ unit }}
        when 'ug/m3' then {{ value }}
        when 'mg/m3' then {{ value }} * 1000
    end
{%- endmacro %}
