resource "google_bigquery_dataset" "raw" {
  dataset_id  = var.raw_dataset_id
  location    = var.region
  description = "Raw layer of aria-er: external tables over the Parquet files in Cloud Storage."

  delete_contents_on_destroy = var.allow_destroy

  depends_on = [google_project_service.required]
}

resource "google_bigquery_dataset" "analytics" {
  dataset_id  = var.analytics_dataset_id
  location    = var.region
  description = "aria-er models built by dbt: staging, intermediate, marts, seeds, snapshots."

  delete_contents_on_destroy = var.allow_destroy

  depends_on = [google_project_service.required]
}

resource "google_bigquery_table" "partitioned" {
  for_each = var.external_tables_enabled ? local.partitioned_tables : {}

  dataset_id          = google_bigquery_dataset.raw.dataset_id
  table_id            = each.key
  deletion_protection = !var.allow_destroy
  schema              = file("${path.module}/schemas/${each.value.schema}.json")

  external_data_configuration {
    source_format = "PARQUET"
    autodetect    = false
    source_uris   = ["${local.raw_uri}/${each.value.path}/*"]

    hive_partitioning_options {
      mode              = "CUSTOM"
      source_uri_prefix = "${local.raw_uri}/${each.value.path}/{year:INTEGER}/{month:INTEGER}"
    }
  }
}

resource "google_bigquery_table" "snapshot" {
  for_each = var.external_tables_enabled ? local.snapshot_tables : {}

  dataset_id          = google_bigquery_dataset.raw.dataset_id
  table_id            = each.key
  deletion_protection = !var.allow_destroy
  schema              = file("${path.module}/schemas/${each.value.schema}.json")

  external_data_configuration {
    source_format = "PARQUET"
    autodetect    = false
    source_uris   = ["${local.raw_uri}/${each.value.path}/*"]
  }
}
