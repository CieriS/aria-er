# Continuous integration

Workflows, fixtures and the rule protecting `main`.

## Continuous integration

Three workflows run on every pull request and on every push to `main`, each with its
dependency cache:

| Workflow | Checks |
|---|---|
| [Rust](../.github/workflows/rust.yml) | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` |
| [Python](../.github/workflows/python.yml) | `ruff format --check`, `ruff check`, `mypy --strict`, `pytest`, for `orchestration` and `dashboard` |
| [dbt](../.github/workflows/dbt.yml), second job | The same models compiled for BigQuery and executed on the BigQuery emulator, results compared with DuckDB (`make bigquery-dialect-check`) |
| [Terraform](../.github/workflows/terraform.yml) | `terraform fmt -check`, `validate`, and `plan` (never `apply`) when GCP settings exist; only on changes under `infra/` |
| [dbt](../.github/workflows/dbt.yml) | `dbt build` on DuckDB with the committed fixtures, twice (full and incremental) |
| [Docker](../.github/workflows/docker.yml) | `docker compose build` of the images `make up` starts; only on changes to the Dockerfiles, the compose file, the ingestor or the Python dependencies |

One more workflow is scheduled rather than triggered by a change:
[Publish data](../.github/workflows/publish-data.yml) runs the whole pipeline against the
live sources once a day and publishes the marts on GitHub Pages ([export.md](export.md)).

No other workflow calls ARPAE or Open-Meteo: sources are mocked behind traits in Rust, and dbt
reads `transform/fixtures/`. `make transform-fixtures` runs the dbt job locally. Actions are
pinned by commit, and Dependabot proposes weekly updates for actions, crates, Python
packages and base images.

### Branch protection

`main` only changes through pull requests that pass the CI. The ruleset `main`
(*Settings → Rules → Rulesets*, target: default branch) is active since 2026-10-12, with no
bypass:

- **A pull request is required** (no direct pushes to `main`).
- **Status checks must pass**, with the branch up to date: `fmt, clippy, test`,
  `orchestration (ruff, mypy, pytest)`, `dashboard (ruff, mypy, pytest)`,
  `dbt build on fixtures (DuckDB)` and `BigQuery dialect (emulator)`.
- **Force pushes and deletion are blocked.**

The Docker and Terraform checks are filtered by path, so they cannot be required: a pull
request that does not touch their files would wait for them forever.

Approvals are not required: this is a single-maintainer repository, so the gate is the CI.
