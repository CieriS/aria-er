locals {
  raw_bucket_name = coalesce(var.raw_bucket_name, "${var.project_id}-aria-er-raw")
  raw_uri         = "gs://${local.raw_bucket_name}/${var.raw_prefix}"

  # Time series written by the ingestor as year=YYYY/month=MM/part-0.parquet.
  partitioned_tables = {
    arpae_measurements         = { path = "arpae/measurements", schema = "measurements" }
    arpae_measurements_archive = { path = "arpae/measurements_archive", schema = "measurements" }
    openmeteo_weather          = { path = "openmeteo/weather", schema = "weather" }
  }

  # Dated snapshots written as extracted_on=YYYY-MM-DD/<file>.parquet. The date is also a
  # column of the file, so the directory is not declared as a partition key.
  snapshot_tables = {
    arpae_stations      = { path = "arpae/stations", schema = "stations" }
    arpae_station_types = { path = "arpae/station_types", schema = "station_types" }
  }
}

resource "google_project_service" "required" {
  for_each = toset(["bigquery.googleapis.com", "storage.googleapis.com", "iam.googleapis.com"])

  service            = each.value
  disable_on_destroy = false
}
