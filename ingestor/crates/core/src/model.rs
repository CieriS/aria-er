use chrono::{DateTime, NaiveDate, Utc};

/// A pollutant as listed in the ARPAE parameter registry.
#[derive(Debug, Clone, PartialEq)]
pub struct Pollutant {
    pub id: u32,
    pub name: String,
    /// Unit as published by the source (e.g. `ug/m3`, `mg/m3`).
    pub unit: String,
}

/// A monitoring station.
#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    /// Canonical numeric code (`7000014`), whatever the source formatting.
    pub id: u32,
    pub name: String,
    pub municipality: String,
    pub province: String,
    pub address: String,
    pub altitude_m: Option<f64>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
}

/// One row of the station registry: a pollutant measured at a station.
#[derive(Debug, Clone, PartialEq)]
pub struct StationSensor {
    pub station: Station,
    pub pollutant: Pollutant,
}

/// Natural key of a measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeasurementKey {
    pub station_id: u32,
    pub pollutant_id: u32,
    pub measured_at: DateTime<Utc>,
}

/// A single raw measurement, normalised only as far as needed to key it.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub station_id: u32,
    pub pollutant_id: u32,
    /// Source reference time converted to UTC.
    pub measured_at: DateTime<Utc>,
    pub value: f64,
    /// Unit of `value` as published; `None` if the pollutant is not in the registry.
    pub unit: Option<String>,
    /// Source validation flag, kept verbatim (provisional vs validated).
    pub validation_flag: String,
    /// Reference time exactly as published by the source.
    pub raw_reftime: String,
}

impl Measurement {
    pub fn key(&self) -> MeasurementKey {
        MeasurementKey {
            station_id: self.station_id,
            pollutant_id: self.pollutant_id,
            measured_at: self.measured_at,
        }
    }
}

/// Inclusive range of calendar days, expressed in the source's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateWindow {
    from: NaiveDate,
    to: NaiveDate,
}

impl DateWindow {
    /// Returns `None` when `from` is after `to`.
    pub fn new(from: NaiveDate, to: NaiveDate) -> Option<Self> {
        (from <= to).then_some(Self { from, to })
    }

    /// Window of `days` days ending on `to` (inclusive). `days == 0` is treated as 1.
    pub fn ending_on(to: NaiveDate, days: u32) -> Self {
        let back = chrono::Days::new(u64::from(days.max(1) - 1));
        Self {
            from: to.checked_sub_days(back).unwrap_or(NaiveDate::MIN),
            to,
        }
    }

    pub fn from(&self) -> NaiveDate {
        self.from
    }

    pub fn to(&self) -> NaiveDate {
        self.to
    }

    pub fn contains(&self, day: NaiveDate) -> bool {
        self.from <= day && day <= self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn window_rejects_inverted_range() {
        assert!(DateWindow::new(d(2026, 8, 2), d(2026, 8, 1)).is_none());
        assert!(DateWindow::new(d(2026, 8, 1), d(2026, 8, 1)).is_some());
    }

    #[test]
    fn window_ending_on_counts_days_inclusively() {
        let w = DateWindow::ending_on(d(2026, 9, 30), 30);
        assert_eq!(w.from(), d(2026, 9, 1));
        assert_eq!(w.to(), d(2026, 9, 30));
        assert!(w.contains(d(2026, 9, 1)) && !w.contains(d(2026, 8, 31)));
    }
}
