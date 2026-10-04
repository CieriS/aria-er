# 0004 — Source and Sink generic over the record type

Status: accepted (phase 4)

## Context

The first version of the traits was written around ARPAE:

```rust
trait Source {
    fn fetch_measurements(&self, window: DateWindow) -> Result<Vec<Measurement>, SourceError>;
    fn fetch_stations(&self) -> Result<Vec<StationSensor>, SourceError>;
}
```

Open-Meteo has neither pollutant measurements nor a station registry: an hour of weather
is a wide row (temperature, wind, precipitation, pressure) at a coordinate.

## Options

1. **Keep the trait and fit weather into `Measurement`**, with invented pollutant ids per
   variable and an empty `fetch_stations`. No trait change, but the domain model lies and
   the wide row has to be rebuilt downstream.
2. **A second trait** (`WeatherSource`). ARPAE untouched, two near-identical traits and a
   duplicated pipeline in the CLI.
3. **One trait with an associated record type.**

## Decision

Option 3:

```rust
trait Source { type Record; fn fetch(&self, window: DateWindow) -> Result<Vec<Self::Record>, SourceError>; }
trait Sink   { type Record; fn write(&self, records: &[Self::Record]) -> Result<WriteReport, SinkError>; }
```

- `ArpaeSource` is a `Source<Record = Measurement>`; its registry is a separate
  `Source<Record = StationSensor>` (`ArpaeSource::stations()`), which ignores the window.
- `OpenMeteoSource` is a `Source<Record = WeatherObservation>`.
- The Parquet upsert became `PartitionedSink<R>`: a record type only declares its key,
  timestamp, schema and column conversion. Measurements and weather share the merge,
  ordering and atomic write.
- The CLI pipeline is one function, `ingest(source, sink, window)`, checked by the compiler
  to pair a source and a sink of the same record type.
- The HTTP transport and retry moved to a small `aq-http` crate, now that two sources use it.

## Consequences

- Existing behaviour and file layout are unchanged; the previous tests pass after adapting
  call sites only.
- Adding a source means a record type, a `Source` impl and a `PartitionedRecord` impl.
- A registry-like source receives a window it does not use; accepted to keep one trait.
