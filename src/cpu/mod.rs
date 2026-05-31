use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetSystemTimes};

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuUsageSnapshot {
    pub percent: Option<f32>,
}

#[derive(Debug, Default)]
pub struct CpuUsageMonitor {
    previous: Option<SystemCpuTimes>,
}

#[derive(Debug, Clone, Copy)]
struct SystemCpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

impl CpuUsageMonitor {
    pub fn sample(&mut self) -> CpuUsageSnapshot {
        let Some(current) = read_system_cpu_times() else {
            return CpuUsageSnapshot { percent: None };
        };

        let percent = self.previous.and_then(|previous| {
            let idle_delta = current.idle.saturating_sub(previous.idle);
            let kernel_delta = current.kernel.saturating_sub(previous.kernel);
            let user_delta = current.user.saturating_sub(previous.user);
            let total_delta = kernel_delta + user_delta;

            if total_delta == 0 {
                None
            } else {
                let used = total_delta.saturating_sub(idle_delta);
                Some(((used as f32 / total_delta as f32) * 100.0).clamp(0.0, 100.0))
            }
        });

        self.previous = Some(current);
        CpuUsageSnapshot { percent }
    }
}

fn read_system_cpu_times() -> Option<SystemCpuTimes> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();

    let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    if ok == 0 {
        return None;
    }

    Some(SystemCpuTimes {
        idle: filetime_to_u64(idle),
        kernel: filetime_to_u64(kernel),
        user: filetime_to_u64(user),
    })
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_filetime_parts() {
        let value = FILETIME {
            dwLowDateTime: 0x89ab_cdef,
            dwHighDateTime: 0x0123_4567,
        };

        assert_eq!(filetime_to_u64(value), 0x0123_4567_89ab_cdef);
    }
}
