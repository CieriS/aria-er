output "raw_bucket" {
  description = "Bucket of the raw layer (AQ_SINK__GCS_BUCKET for the ingestor)."
  value       = google_storage_bucket.raw.name
}

output "raw_dataset" {
  description = "Dataset with the external tables (dbt variable raw_dataset)."
  value       = google_bigquery_dataset.raw.dataset_id
}

output "analytics_dataset" {
  description = "Dataset of the dbt models."
  value       = google_bigquery_dataset.analytics.dataset_id
}

output "ingestor_service_account" {
  description = "Identity the ingestor runs as."
  value       = google_service_account.ingestor.email
}

output "dbt_service_account" {
  description = "Identity dbt runs as."
  value       = google_service_account.dbt.email
}
