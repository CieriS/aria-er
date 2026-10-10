# Running on Google Cloud

The same pipeline with Cloud Storage as the raw layer and BigQuery as the warehouse, sized
to stay inside the GCP free tier.

> **Status.** The pipeline has **not yet been run on a real GCP project**. What is verified
> without one:
>
> - **BigQuery SQL**: `make bigquery-dialect-check` compiles every model for the BigQuery
>   adapter, executes it on the open-source BigQuery emulator (whose SQL analyser is the one
>   BigQuery uses) together with 104 tests, and finds the results identical to the DuckDB
>   build of the same fixtures. It caught three errors the syntax check had missed.
> - **Storage backend**: tested on an in-memory object store, byte-identical to local files.
> - **Terraform**: `fmt` and `validate`.
>
> Still unverified until the first real run: writing to an actual bucket, the external
> tables over Cloud Storage, IAM, dbt's own BigQuery materialisations (merge, snapshot,
> seed load) and the execution of the window frame of the 8-hour ozone mean, which the
> emulator can analyse but not run.

## What runs where

```
aq-ingest (--features gcs) ──► gs://<project>-aria-er-raw/raw/...   Parquet, same layout as raw/
                                          │
                     BigQuery dataset aria_er_raw: external tables over those files
                                          │
                 dbt --target bigquery ──► BigQuery dataset aria_er: staging, intermediate, marts
```

Nothing runs permanently in the cloud: the ingestor and dbt are started from a machine or
a scheduler with credentials, exactly as locally. There is no VM, cluster or always-on
service, which is what keeps the cost at zero.

| Piece | Where |
|---|---|
| Bucket, datasets, external tables, service accounts | [`infra/`](../infra) (Terraform) |
| Storage backend of the ingestor | `sink-parquet`, Cargo feature `gcs` |
| BigQuery target and dialect macros | [`transform/profiles.yml`](../transform/profiles.yml), [`transform/macros/dialect.sql`](../transform/macros/dialect.sql) |
| Comparison between the two warehouses | [`transform/scripts/compare_targets.py`](../transform/scripts/compare_targets.py) |
| BigQuery SQL check without GCP | [`transform/scripts/check_bigquery_dialect.py`](../transform/scripts/check_bigquery_dialect.py), `make bigquery-dialect-check` |

## Setup

Requirements: a GCP project with billing enabled, `gcloud`, and Terraform 1.9 or later
(or the `hashicorp/terraform` Docker image).

```bash
export AQ_GCP_PROJECT=<project id>
export AQ_GCS_BUCKET=$AQ_GCP_PROJECT-aria-er-raw
gcloud auth login
gcloud auth application-default login
gcloud config set project $AQ_GCP_PROJECT
```

**1. State bucket** (once, by hand: Terraform cannot store its state in a bucket it has
not created yet):

```bash
gcloud storage buckets create gs://$AQ_GCP_PROJECT-aria-er-tfstate \
  --location=us-central1 --uniform-bucket-level-access --public-access-prevention
```

**2. Infrastructure, first pass** (bucket, datasets, service accounts):

```bash
cd infra
cp terraform.tfvars.example terraform.tfvars      # set project_id
terraform init -backend-config="bucket=$AQ_GCP_PROJECT-aria-er-tfstate"
terraform apply
```

**3. First ingestion** into the bucket (from the repository root):

```bash
make cloud-ingest FROM=2026-08-01 TO=2026-08-31
```

**4. Infrastructure, second pass**: BigQuery can only create an external table when files
exist under its prefix, so the tables are enabled after the first ingestion. Set
`external_tables_enabled = true` in `terraform.tfvars`, then `terraform apply` again.

**5. Models and check**:

```bash
make cloud-transform      # dbt build --target bigquery
make transform            # the same models locally, on the same data
make cloud-compare        # mart_exceedances_yearly must be identical in the two warehouses
```

### Credentials

Terraform creates two service accounts and no keys:

| Account | Can do |
|---|---|
| `aria-er-ingestor` | Read and write objects of the raw bucket (`roles/storage.objectUser` on that bucket only) |
| `aria-er-dbt` | Run query jobs, read the raw dataset and its files, write the analytics dataset |

The steps above use your own user credentials, which is enough to run the pipeline once.
To run it as the service accounts (a scheduler, CI), create a key for each, keep it out of
the repository, and point `GOOGLE_APPLICATION_CREDENTIALS` at it:

```bash
gcloud iam service-accounts keys create ~/aria-er-ingestor.json \
  --iam-account aria-er-ingestor@$AQ_GCP_PROJECT.iam.gserviceaccount.com
```

### Terraform plan in CI

The [Terraform workflow](../.github/workflows/terraform.yml) always checks `fmt` and
`validate`. It also runs `terraform plan` — never `apply` — when the repository has:

- variable `GCP_PROJECT_ID` and `GCP_TERRAFORM_STATE_BUCKET`;
- secret `GCP_TERRAFORM_PLAN_KEY`: the key of a read-only identity (project `Viewer`,
  `Security Reviewer`, and object viewer on the state bucket).

Without them the plan job is skipped.

## Cost

Estimated cost: **0 € per month**, inside the free tier, with cents at worst.

| Resource | Use by aria-er | Free tier (per month) |
|---|---|---|
| Cloud Storage, regional storage in `us-central1` | about 6 MB of Parquet (145 files) for ten years of the sample archive plus the recent weeks | 5 GB |
| Cloud Storage, write operations (class A) | a daily run rewrites under 10 objects: about 300 | 5,000 |
| Cloud Storage, read operations (class B) | each dbt build reads the files behind the external tables, about 1,000 reads: about 30,000 with a daily build | 50,000 |
| BigQuery storage | under 100 MB of native tables (the local DuckDB file is 53 MB) | 10 GB |
| BigQuery queries | a build scans well under 2 GB: about 60 GB with a daily build | 1 TB |
| Compute | none | — |

What keeps it sustainable:

- **Region**: the Cloud Storage free tier only covers `us-east1`, `us-west1` and
  `us-central1`. The data is public, so there is no residency constraint.
- **No soft delete, no versioning** on the bucket: the ingestor rewrites whole partition
  files, and retained copies would be billed.
- **`maximum_bytes_billed`** of 10 GB per query in the dbt profile: a runaway query is
  refused instead of billed.
- **The read operations are the closest limit.** They grow with the number of archive files
  and with how often dbt runs. Beyond the free tier they cost a fraction of a cent per
  thousand; if the archive grows, the fix is to load it into a native table once instead of
  reading it through an external table at every build.

The figures of the free tier are those published by Google at the time of writing: check
the [Cloud Storage](https://cloud.google.com/storage/pricing) and
[BigQuery](https://cloud.google.com/bigquery/pricing) pricing pages, and set a budget alert
on the billing account before the first apply.

## Teardown

Everything created by this project, in reverse order:

```bash
# 1. Bucket content, datasets, tables and service accounts.
cd infra
terraform apply -var allow_destroy=true       # lets the bucket and datasets be emptied
terraform destroy -var allow_destroy=true

# 2. Keys created by hand, if any.
gcloud iam service-accounts keys list \
  --iam-account aria-er-ingestor@$AQ_GCP_PROJECT.iam.gserviceaccount.com

# 3. The state bucket, created by hand.
gcloud storage rm -r gs://$AQ_GCP_PROJECT-aria-er-tfstate

# 4. Local credentials.
gcloud auth application-default revoke
```

Service accounts are destroyed by Terraform together with their keys. If the project was
created only for this, deleting the project removes everything at once:
`gcloud projects delete $AQ_GCP_PROJECT`.
