variable "project_id" {
  description = "GCP project that hosts the platform."
  type        = string
}

variable "region" {
  description = "Region of the bucket and of the BigQuery datasets. The Cloud Storage free tier only applies to us-east1, us-west1 and us-central1."
  type        = string
  default     = "us-central1"
}

variable "raw_bucket_name" {
  description = "Name of the bucket holding the raw layer. Defaults to <project>-aria-er-raw."
  type        = string
  default     = null
}

variable "raw_prefix" {
  description = "Prefix of the raw layer inside the bucket, as written by the ingestor."
  type        = string
  default     = "raw"
}

variable "raw_dataset_id" {
  description = "BigQuery dataset exposing the raw layer as external tables."
  type        = string
  default     = "aria_er_raw"
}

variable "analytics_dataset_id" {
  description = "BigQuery dataset the dbt models are built in."
  type        = string
  default     = "aria_er"
}

variable "external_tables_enabled" {
  description = "Create the external tables over the raw layer. Leave false for the first apply: BigQuery needs at least one file under each prefix, so enable it after the first ingestion."
  type        = bool
  default     = false
}

variable "allow_destroy" {
  description = "Let `terraform destroy` delete the bucket and the datasets with their content."
  type        = bool
  default     = false
}
