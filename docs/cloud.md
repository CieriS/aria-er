# Running on Google Cloud

The same pipeline with Cloud Storage as the raw layer and BigQuery as the warehouse, sized
to stay inside the GCP free tier.

> **Status.** Run end to end on a real GCP project on 2026-10-21: two Terraform passes,
> ingestion of August 2026 plus the ten-year sample archive into the bucket, `dbt build` on
> BigQuery (128 passed, 1 warning: the flag on negative measurements, as in DuckDB), and all
> five marts identical to a DuckDB build of the same files. See [First run](#first-run).
>
> Also checked without GCP, on every pull request: `make bigquery-dialect-check` compiles every
> model for the BigQuery adapter, executes it on the open-source BigQuery emulator and compares
> the results with the DuckDB build of the same fixtures.

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
make cloud-compare        # mart_exceedances_yearly must be identical in the two warehouses
```

`cloud-compare` reads the local warehouse, so the two must have been built from the same
files. When the local `raw/` holds more than the bucket (the usual case), build a separate
DuckDB from a copy of the bucket and compare against that one:

```bash
gcloud storage rsync -r gs://$AQ_GCS_BUCKET /tmp/aria-er-bucket
cd transform
AQ_DUCKDB_PATH=/tmp/aria-er-bucket.duckdb uv run dbt build --profiles-dir . \
  --vars "{raw_dir: '/tmp/aria-er-bucket/raw'}"
AQ_GCP_PROJECT=$AQ_GCP_PROJECT uv run --group cloud python scripts/compare_targets.py \
  --duckdb /tmp/aria-er-bucket.duckdb --mart mart_exceedances_yearly
```

### Login

`gcloud auth login` and `gcloud auth application-default login` end with a redirect to
`http://localhost:<port>`. A browser in HTTPS-only mode shows a "connection is not secure"
page there: continuing is safe, the address is `gcloud` itself on the same machine. On the
consent page of the second command the Cloud Platform permission has to be ticked, otherwise
the command fails with `cloud-platform scope is required but not consented`.

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
| BigQuery storage | 128 MB of native tables, measured on the first run | 10 GB |
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

## First run

On 2026-10-21, project `aria-er`, `us-central1`, following the setup above:

| Step | Result |
|---|---|
| `terraform apply`, first pass | 13 resources: APIs, bucket, two datasets, two service accounts, IAM bindings |
| `make cloud-ingest FROM=2026-08-01 TO=2026-08-31` | 4 MB of Parquet in the bucket: the month, its weather, station snapshots and the ten-year archive (130 partitions) |
| `terraform apply`, second pass | 5 external tables |
| `make cloud-transform` | 129 nodes: 128 passed, 1 warning (`warn_negative_measurements`, the same in DuckDB) |
| `make cloud-transform`, again | the incremental model and the snapshot run as `MERGE`; marts unchanged |
| Comparison with DuckDB on the same files | identical: `mart_exceedances_yearly` (339 rows), `mart_pollutant_trend` (330), `mart_traffic_vs_background` (581), `mart_weather_correlation` (48), `mart_data_completeness` (112,071) |

Nothing that the emulator could not check failed: the GCS backend wrote to a real bucket,
the external tables read the hive-partitioned files, the service accounts and bindings were
created, and dbt's BigQuery materialisations (seed load, snapshot, incremental model) ran on
the first attempt, the incremental merge and the snapshot on the second build.

**Cost of the run:** 278 BigQuery jobs (two builds and the comparisons), 0.84 GB processed
and 2.95 GB billed (BigQuery bills at least 10 MB per query), against 1 TB free per month;
4 MB in Cloud Storage. No charge.

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
