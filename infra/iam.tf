# Two identities with the minimum each needs. No keys are created here: a key in the
# Terraform state would be a secret in a bucket (see docs/cloud.md for how to authenticate).

resource "google_service_account" "ingestor" {
  account_id   = "aria-er-ingestor"
  display_name = "aria-er ingestor"
  description  = "Writes the raw layer to Cloud Storage."

  depends_on = [google_project_service.required]
}

resource "google_service_account" "dbt" {
  account_id   = "aria-er-dbt"
  display_name = "aria-er dbt"
  description  = "Builds the dbt models in BigQuery from the raw layer."

  depends_on = [google_project_service.required]
}

# The ingestor reads and rewrites objects of the raw bucket, and nothing else.
resource "google_storage_bucket_iam_member" "ingestor_objects" {
  bucket = google_storage_bucket.raw.name
  role   = "roles/storage.objectUser"
  member = google_service_account.ingestor.member
}

# dbt runs query jobs in the project...
resource "google_project_iam_member" "dbt_jobs" {
  project = var.project_id
  role    = "roles/bigquery.jobUser"
  member  = google_service_account.dbt.member
}

# ...reads the external tables and the files behind them...
resource "google_bigquery_dataset_iam_member" "dbt_raw" {
  dataset_id = google_bigquery_dataset.raw.dataset_id
  role       = "roles/bigquery.dataViewer"
  member     = google_service_account.dbt.member
}

resource "google_storage_bucket_iam_member" "dbt_objects" {
  bucket = google_storage_bucket.raw.name
  role   = "roles/storage.objectViewer"
  member = google_service_account.dbt.member
}

# ...and owns the tables of the analytics dataset.
resource "google_bigquery_dataset_iam_member" "dbt_analytics" {
  dataset_id = google_bigquery_dataset.analytics.dataset_id
  role       = "roles/bigquery.dataEditor"
  member     = google_service_account.dbt.member
}
