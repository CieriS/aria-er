# 0007 — Streaming is not justified for these data

Status: proposed (phase 8) — awaiting approval. If approved, the phase closes here.

## Question

Phase 8 is optional: build a streaming variant (Redpanda, a producer publishing new
measurements, a consumer writing the raw layer) only if the data and the use cases call
for it. This record answers that before any code is written.

## Evidence

Measured on 2026-10-10 against the live sources.

**How often the sources change**

| Source | What ARPAE states | What was observed |
|---|---|---|
| CKAN datastore (the ingestor's source) | "Updated daily, when the bulletin is issued" (resource description) | Newest row dated 2026-09-17: not updated for 23 days |
| REST archive `qa_archivio_dati_public` (appeared 2026-10-09) | — | Each record carries its publication time. Hourly values of 9 October were published in batches at 08:32, 09:55 and 12:01 GMT, covering 107 hours (the initial load), 1 and 2 hours; nothing in the following 18 hours |
| PM10, PM2.5 | Daily means | One value per station per day, published the day after |
| Station registry, bulletin | Daily | One version per day |

The freshest value ever observed was published 1.5 hours after the end of its hour, in a
batch, flagged as not validated. Both sources are polled HTTP APIs; a query on the
datastore takes about 40 seconds. Neither offers a push interface (queue, webhook,
server-sent events): a "producer" could only poll the same APIs the batch polls.

**How long a value keeps changing**

ARPAE validates in stages, recorded in the validation flag: `T` temporary (automatic
check), `G` daily, `M` monthly, `S` six-monthly. A value can therefore be revised for
months. In the datastore, September is validated daily and August monthly. What makes
the dataset correct is re-reading a window of past days, which the batch already does
(30 days, upsert by natural key), not reading each value sooner.

**What the use cases need**

| Use case | Grain | Freshness that changes the answer |
|---|---|---|
| Exceedances of legal limits | Daily mean, hourly mean, daily 8-hour maximum, counted per year | A day |
| Multi-year trend | Year | Months |
| Traffic against background | Month | Weeks |
| Weather correlation | Day, by season | Weeks |
| Data completeness | Day | A day |
| Freshness alarm | — | Fires at 48 hours |

No question the platform answers changes with data that is minutes rather than hours old,
and the legal metrics are themselves defined on whole hours and days.

## Options

1. **Streaming**: Redpanda in the stack, a producer polling ARPAE and publishing to a
   topic, a consumer writing Parquet with the existing upsert, integration tests with
   testcontainers.
2. **A more frequent batch**: run the existing schedule every hour instead of every day.
3. **Nothing**: keep the daily schedule.

## Decision

Option 3, with option 2 as the path if fresher data is ever wanted. No streaming.

- The source is a batch. Putting a broker between a poller and a file writer adds an
  always-on service, a second code path and a new failure mode, and by the phase's own
  acceptance criterion would produce exactly the output the batch already produces.
- The latency floor is set upstream (1.5 hours at best, usually far more) and by
  validation (months). A broker cannot lower either.
- An always-on broker contradicts the cloud design, which has no compute and costs nothing
  (ADR 0006).
- Option 2 costs one line: the ingestion is idempotent and partitions are only rewritten
  when their content changes, so running it hourly is safe. It is not done now because no
  use case needs it and the datastore has not been updating at all.

## What would change the answer

- ARPAE (or another source) offers a push feed, or sensors with sub-hourly readings.
- A use case with a deadline in minutes, such as alerting when a threshold is crossed.
- Several independent consumers needing the same events, where a log would decouple them.

## Consequences

- Phase 8 closes with this record; no Redpanda, producer or consumer is added.
- The measurements made for this record exposed a more pressing issue than latency: the
  ingestor's near-real-time source has stopped updating while ARPAE publishes fresh data on
  a new API (see `docs/data.md` and `docs/limitations.md`). Moving the ingestion to that
  API is the next piece of work worth doing, and it is a batch change.
