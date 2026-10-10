resource "google_storage_bucket" "raw" {
  name     = local.raw_bucket_name
  location = var.region

  storage_class               = "STANDARD"
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  force_destroy               = var.allow_destroy

  # The ingestor rewrites whole partition files: keeping old generations or soft-deleted
  # copies would only add storage cost.
  versioning {
    enabled = false
  }

  soft_delete_policy {
    retention_duration_seconds = 0
  }

  depends_on = [google_project_service.required]
}
