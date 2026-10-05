"""Rebuild transform/seeds/station_classification.csv from the ARPAE daily bulletin.

The station registry does not say whether a station measures traffic or background;
the daily air quality bulletin does (`tipostazione`). Standard library only:

    python3 scripts/update_station_classification.py
"""

import csv
import json
import sys
import urllib.request
from pathlib import Path

BULLETIN_URL = "https://apps.arpae.it/REST/bollettini_qa"
SEED = Path(__file__).resolve().parents[1] / "transform/seeds/station_classification.csv"

STATION_TYPES = {"traffico": "traffic", "fondo": "background", "industriale": "industrial"}
AREA_TYPES = {
    "urbana": "urban",
    "suburbana": "suburban",
    "rurale": "rural",
    "remoto": "remote",
    "remota": "remote",
}


def fetch(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


def classify(label: str) -> tuple[str, str]:
    """Split a bulletin label such as 'Urbana Traffico' into (station_type, area_type)."""
    words = label.lower().split()
    station_type = next((STATION_TYPES[w] for w in words if w in STATION_TYPES), None)
    area_type = next((AREA_TYPES[w] for w in words if w in AREA_TYPES), "")
    # Labels outside the three exposure types (e.g. 'Locale') are kept as 'other'.
    return station_type or "other", area_type


def latest_bulletin() -> dict:
    """The most recent bulletin: the last item of the last page."""
    first = fetch(f"{BULLETIN_URL}?t=json")
    last_page = first["_links"]["last"]["href"].split("page=")[1].split("&")[0]
    return fetch(f"{BULLETIN_URL}?page={last_page}&t=json")["_items"][-1]


def stations(bulletin: dict) -> dict[int, tuple[str, str, str]]:
    rows = {}
    for versions in bulletin.values():
        if not (isinstance(versions, list) and versions and isinstance(versions[0], dict)):
            continue
        for station in versions[-1].get("data", []):
            label = station["tipostazione"].strip()
            rows[int(station["idstazione"])] = (*classify(label), label)
    return rows


def main() -> int:
    bulletin = latest_bulletin()
    rows = stations(bulletin)
    source = f"ARPAE daily air quality bulletin {bulletin['_id']}"
    with SEED.open("w", newline="") as file:
        writer = csv.writer(file, lineterminator="\n")
        writer.writerow(
            ["station_id", "station_type", "area_type", "source_label", "classification_source"]
        )
        for station_id in sorted(rows):
            writer.writerow([station_id, *rows[station_id], source])
    print(f"{len(rows)} stations written to {SEED}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
