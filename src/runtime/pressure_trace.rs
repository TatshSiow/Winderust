//! Optional pressure diagnostics for the local runtime benchmark.

use std::{
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use crate::config::AdaptivePressureSource;

const MAX_EVENTS: usize = 4_096;
static RECORDING: OnceLock<Mutex<Option<Recording>>> = OnceLock::new();

pub(crate) struct PressureTrace(&'static Mutex<Option<Recording>>);

struct Recording {
    start: Instant,
    rows: Vec<Row>,
}

#[derive(Serialize)]
struct Row {
    elapsed_us: u128,
    utc_unix_us: Option<u128>,
    #[serde(flatten)]
    event: PressureEvent,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PressureEvent {
    Gate {
        source: AdaptivePressureSource,
        system_cpu_percent: Option<f32>,
        foreground_cpu_percent: Option<f32>,
        system_threshold: u8,
        foreground_threshold: u8,
        recovery_band: u8,
        was_active: bool,
        active: bool,
        decision: &'static str,
    },
    Reset {
        reason: String,
    },
}

impl PressureTrace {
    pub(crate) fn start() -> Result<Self, String> {
        let recorder = RECORDING.get_or_init(|| Mutex::new(None));
        let mut slot = recorder.lock().map_err(|error| error.to_string())?;
        if slot.is_some() {
            return Err("Adaptive Engine pressure trace is already active".to_owned());
        }
        *slot = Some(Recording {
            start: Instant::now(),
            rows: Vec::new(),
        });
        Ok(Self(recorder))
    }

    pub(crate) fn finish(self) -> Result<String, String> {
        let slot = self.0.lock().map_err(|error| error.to_string())?;
        let recording = slot
            .as_ref()
            .ok_or_else(|| "Adaptive Engine pressure trace is missing".to_owned())?;
        if recording.rows.len() > MAX_EVENTS {
            return Err("Adaptive Engine pressure trace exceeded its capacity".to_owned());
        }
        serde_json::to_string_pretty(&recording.rows).map_err(|error| error.to_string())
    }
}

impl Drop for PressureTrace {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }
}

pub(crate) fn record(event: impl FnOnce() -> PressureEvent) {
    let Some(recorder) = RECORDING.get() else {
        return;
    };
    let Ok(mut slot) = recorder.lock() else {
        return;
    };
    if let Some(recording) = slot
        .as_mut()
        .filter(|recording| recording.rows.len() <= MAX_EVENTS)
    {
        recording.rows.push(Row {
            elapsed_us: recording.start.elapsed().as_micros(),
            utc_unix_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|time| time.as_micros()),
            event: event(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "pressure tracing is process-wide; run in isolation"]
    fn trace_is_exclusive_bounded_and_releases_the_recording() -> Result<(), String> {
        record(|| panic!("events should not be built outside a recording"));
        let trace = PressureTrace::start()?;
        assert!(PressureTrace::start().is_err());
        record(|| PressureEvent::Reset {
            reason: "settings changed".into(),
        });
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&trace.finish()?).map_err(|error| error.to_string())?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["kind"], "reset");
        assert_eq!(rows[0]["reason"], "settings changed");
        assert!(rows[0]["elapsed_us"].is_u64());

        let trace = PressureTrace::start()?;
        for _ in 0..=MAX_EVENTS {
            record(|| PressureEvent::Reset {
                reason: String::new(),
            });
        }
        record(|| panic!("events should not be built after the trace overflows"));
        assert_eq!(
            trace.finish(),
            Err("Adaptive Engine pressure trace exceeded its capacity".into())
        );
        assert_eq!(PressureTrace::start()?.finish()?, "[]");
        Ok(())
    }
}
