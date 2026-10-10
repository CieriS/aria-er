terraform {
  required_version = ">= 1.9"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 7.0"
    }
  }

  # Remote state in a GCS bucket created once by hand (see docs/cloud.md):
  #   terraform init -backend-config="bucket=<state bucket>"
  backend "gcs" {
    prefix = "aria-er/terraform"
  }
}

provider "google" {
  project = var.project_id
  region  = var.region
}
