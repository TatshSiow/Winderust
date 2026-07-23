use crate::ui::app::*;

pub(in crate::ui::app) fn cpu_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(in crate::ui::app) fn cpu_frequency_label(frequency_mhz: Option<u32>) -> String {
    frequency_mhz
        .map(|frequency_mhz| {
            if frequency_mhz >= 1_000 {
                format!("{:.2} GHz", frequency_mhz as f64 / 1_000.0)
            } else {
                format!("{frequency_mhz} MHz")
            }
        })
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(in crate::ui::app) fn memory_usage_label(percent: Option<f32>) -> String {
    percent
        .map(|percent| format!("{percent:.1}%"))
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(in crate::ui::app) fn memory_usage_value_label(snapshot: MemoryUsageSnapshot) -> String {
    match (snapshot.used_physical_bytes, snapshot.total_physical_bytes) {
        (Some(used), Some(total)) => format_memory_used_total(used, total),
        _ => t!("home.collecting").to_string(),
    }
}

pub(in crate::ui::app) fn memory_cache_value_label(snapshot: MemoryUsageSnapshot) -> String {
    snapshot
        .cached_physical_bytes
        .map(format_memory_capacity)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(in crate::ui::app) fn memory_cache_percent(snapshot: MemoryUsageSnapshot) -> Option<f32> {
    memory_bytes_percent(
        snapshot.cached_physical_bytes,
        snapshot.total_physical_bytes,
    )
}

pub(in crate::ui::app) fn refresh_due(
    now: Instant,
    next_refresh: &mut Instant,
    interval: Duration,
) -> bool {
    if now < *next_refresh {
        return false;
    }

    *next_refresh = now + interval;
    true
}

pub(in crate::ui::app) fn active_plan_guid(plans: &[PowerPlan]) -> Option<&str> {
    plans
        .iter()
        .find(|plan| plan.active)
        .map(|plan| plan.guid.as_str())
}

pub(in crate::ui::app) fn memory_bytes_percent(
    bytes: Option<u64>,
    total_bytes: Option<u64>,
) -> Option<f32> {
    let bytes = bytes?;
    let total_bytes = total_bytes?;
    if total_bytes == 0 {
        return None;
    }

    Some(((bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 100.0) as f32)
}

pub(in crate::ui::app) fn io_usage_label(bytes_per_second: Option<f64>) -> String {
    bytes_per_second
        .map(format_bytes_per_second)
        .unwrap_or_else(|| t!("home.collecting").to_string())
}

pub(in crate::ui::app) fn format_memory_used_total(used_bytes: u64, total_bytes: u64) -> String {
    let used = memory_capacity_parts(used_bytes);
    let total = memory_capacity_parts(total_bytes);

    if used.unit == total.unit && used.unit != "B" {
        format!(
            "{} / {} {}",
            format_capacity_number(used.value),
            format_capacity_number(total.value),
            used.unit
        )
    } else {
        format!(
            "{} / {}",
            format_memory_capacity(used_bytes),
            format_memory_capacity(total_bytes)
        )
    }
}

pub(in crate::ui::app) fn format_memory_capacity(bytes: u64) -> String {
    let capacity = memory_capacity_parts(bytes);
    if capacity.unit == "B" {
        format!("{} B", bytes)
    } else {
        format!(
            "{} {}",
            format_capacity_number(capacity.value),
            capacity.unit
        )
    }
}

pub(in crate::ui::app) fn format_capacity_number(value: f64) -> String {
    format!("{value:.1}")
}

pub(in crate::ui::app) fn memory_capacity_parts(bytes: u64) -> MemoryCapacityParts {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= TIB {
        MemoryCapacityParts {
            value: bytes / TIB,
            unit: "TB",
        }
    } else if bytes >= GIB {
        MemoryCapacityParts {
            value: bytes / GIB,
            unit: "GB",
        }
    } else if bytes >= MIB {
        MemoryCapacityParts {
            value: bytes / MIB,
            unit: "MB",
        }
    } else if bytes >= KIB {
        MemoryCapacityParts {
            value: bytes / KIB,
            unit: "KB",
        }
    } else {
        MemoryCapacityParts {
            value: bytes,
            unit: "B",
        }
    }
}

pub(in crate::ui::app) fn format_bytes_per_second(bytes_per_second: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    if bytes_per_second >= GIB {
        format!("{:.1} GB/s", bytes_per_second / GIB)
    } else if bytes_per_second >= MIB {
        format!("{:.1} MB/s", bytes_per_second / MIB)
    } else if bytes_per_second >= KIB {
        format!("{:.1} KB/s", bytes_per_second / KIB)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}
