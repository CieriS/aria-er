# 0009 — The marts are published as a daily JSON export on GitHub Pages

Status: accepted (2026-10-21).

## Question

Seeing the results of aria-er today needs a clone and Docker; the README only has a
recording. The results are to be shown on the aria-er case study of the portfolio
(cieri.vercel.app), in the portfolio's own design. What does this repository publish, where,
and how does the portfolio read it?

## Decision

- **This repository publishes data, not pages.** A scheduled workflow runs the pipeline once
  a day (ingestion, `dbt build` with its tests) and exports the five marts to small JSON
  files plus a `meta.json` with the refresh time, the period covered and the data credits.
  Charts and design belong to the portfolio.
- **On GitHub Pages**, deployed by the same workflow: no external account and no token,
  and Pages answers any origin (`Access-Control-Allow-Origin: *`).
- **The portfolio reads the files in the browser**, adding the Pages origin to its CSP
  `connect-src`. The data are as fresh as the last run, the portfolio stays a static site,
  and if the export cannot be reached the case study still renders, without that section.
- **A versioned contract.** Files live under `data/v1/`; a change that breaks a reader
  goes to `data/v2/` while `v1` keeps being published until the portfolio has moved.
  The contract is documented in [export.md](../export.md).
- **Shaped for charts, not a dump of the warehouse.** Four marts are small and go out
  whole; `mart_data_completeness` (about 180,000 daily rows) goes out per station,
  pollutant and month. Every file is checked against a size limit.
- **The raw layer is cached between runs**, so a run only fetches the default window and
  the archive is not reloaded from scratch.

## Alternatives

- **Vercel for a separate site.** Preview deploys per pull request, but its build cannot
  run the pipeline (no persistent raw layer, minutes of calls to ARPAE on every deploy).
  The data would still be computed in Actions and pushed with a token. Not needed once
  the pages live in the portfolio.
- **A static site generated here** (Observable Framework, exported Altair charts): a second
  design to keep coherent with the portfolio. Rejected in favour of the case study.
- **Portfolio reading the export at build time**, rebuilt daily by a deploy hook: the
  numbers would be in the prerendered HTML, at the price of a production deploy a day
  and a secret in this repository. Kept as a fallback if reading in the browser proves
  a problem.

## Consequences

- When ARPAE fails (502) or a test fails, the workflow fails visibly and Pages keeps the
  last good export; `meta.json` says how old it is.
- A public URL now depends on the shape of the marts: renaming a column is a contract
  change, caught by the export tests.
- GitHub Pages has to be enabled for the repository with "GitHub Actions" as its source
  (a repository setting, done once by hand).
