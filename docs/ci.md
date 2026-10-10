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

No workflow calls ARPAE or Open-Meteo: sources are mocked behind traits in Rust, and dbt
reads `transform/fixtures/`. `make transform-fixtures` runs the dbt job locally. Actions are
pinned by commit, and Dependabot proposes weekly updates for actions, crates, Python
packages and base images.

### Branch protection

`main` only changes through pull requests that pass the CI. The rule to set under
*Settings → Rules → Rulesets* (target: default branch):

- **Require a pull request before merging** (no direct pushes to `main`).
- **Require status checks to pass**, with the branch up to date, for these checks:
  `fmt, clippy, test`, `orchestration (ruff, mypy, pytest)`, `dashboard (ruff, mypy, pytest)`
  and `dbt build on fixtures (DuckDB)`.
- **Block force pushes** and branch deletion.

Approvals are not required: this is a single-maintainer repository, so the gate is the CI.
