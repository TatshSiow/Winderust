use std::path::Path;

pub(crate) use crate::platform::windows::interrupt_timing::{
    is_worker_process, recover_at_startup, run_worker_if_requested, InterruptKind,
    InterruptTimingCapture, InterruptTimingError, InterruptTimingReport,
};

pub(crate) fn export_report(path: &Path, report: &InterruptTimingReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    crate::config::storage::write_bytes_atomically(path, &bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn report_export_preserves_warnings_driver_rows_and_cpu_availability() {
        let path = std::env::temp_dir().join(format!(
            "winderust-interrupt-timing-export-{}.json",
            std::process::id()
        ));
        let mut fixture = serde_json::json!({
            "events_lost": 1,
            "buffers_lost": 2,
            "invalid_intervals": 3,
            "incomplete_intervals": 4,
            "invalid_image_records": 5,
            "unresolved_attribution": 6,
            "modules": [
                {"kind": "Threaded DPC", "module": "測試.sys", "count": 7,
                 "total_inclusive_ms": 2.5, "max_inclusive_ms": 0.75},
                {"kind": "ISR", "module": null, "count": 1,
                 "total_inclusive_ms": 0.25, "max_inclusive_ms": 0.25}
            ],
            "per_cpu": null
        });
        let initial: InterruptTimingReport = serde_json::from_value(fixture.clone()).unwrap();
        export_report(&path, &initial).unwrap();
        let saved =
            || serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved(), fixture);

        fixture["per_cpu"] = serde_json::json!([
            {"cpu": 0, "dpc_count": 0, "isr_count": 0, "dpc_ms": 0.0, "isr_ms": 0.0},
            {"cpu": 1, "dpc_count": 0, "isr_count": 1, "dpc_ms": 0.0, "isr_ms": 0.25}
        ]);
        let updated: InterruptTimingReport = serde_json::from_value(fixture.clone()).unwrap();
        export_report(&path, &updated).unwrap();
        assert_eq!(saved(), fixture);
        assert!(export_report(&path.join("report.json"), &initial).is_err());
        assert_eq!(saved(), fixture);
        fs::remove_file(path).unwrap();
    }
}
