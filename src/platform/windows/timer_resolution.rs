#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerResolutionInfo {
    pub(crate) maximum_100ns: u32,
    pub(crate) minimum_100ns: u32,
}

pub(crate) trait TimerResolutionPlatform {
    fn query(&self) -> Result<TimerResolutionInfo, String>;
    fn request(&self, desired_100ns: u32) -> Result<u32, String>;
    fn release(&self, active_100ns: u32) -> Result<(), String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct WindowsTimerResolutionPlatform;

impl TimerResolutionPlatform for WindowsTimerResolutionPlatform {
    fn query(&self) -> Result<TimerResolutionInfo, String> {
        let mut caps = TimeCaps::default();
        // SAFETY: caps is writable for exactly the supplied TimeCaps size and the FFI declaration
        // matches timeGetDevCaps.
        let result = unsafe {
            time_get_dev_caps(&mut caps as *mut _, std::mem::size_of::<TimeCaps>() as u32)
        };
        mm_result("timeGetDevCaps", result)?;

        let min_period_ms = caps.period_min.max(1);
        let max_period_ms = caps.period_max.max(min_period_ms);
        Ok(TimerResolutionInfo {
            maximum_100ns: period_ms_to_100ns(max_period_ms),
            minimum_100ns: period_ms_to_100ns(min_period_ms),
        })
    }

    fn request(&self, desired_100ns: u32) -> Result<u32, String> {
        let period_ms = resolution_100ns_to_period_ms(desired_100ns);
        // SAFETY: period_ms is normalized to a positive millisecond period accepted by winmm.
        let result = unsafe { time_begin_period(period_ms) };
        mm_result("timeBeginPeriod", result).map(|()| period_ms_to_100ns(period_ms))
    }

    fn release(&self, active_100ns: u32) -> Result<(), String> {
        let period_ms = resolution_100ns_to_period_ms(active_100ns);
        // SAFETY: period_ms is the same normalized value used for the matching begin request.
        let result = unsafe { time_end_period(period_ms) };
        mm_result("timeEndPeriod", result)
    }
}

fn resolution_100ns_to_period_ms(value_100ns: u32) -> u32 {
    value_100ns.div_ceil(10_000).max(1)
}

fn period_ms_to_100ns(period_ms: u32) -> u32 {
    period_ms.saturating_mul(10_000)
}

fn mm_result(operation: &str, result: u32) -> Result<(), String> {
    if result == MMSYSERR_NOERROR {
        Ok(())
    } else {
        Err(format!("{operation} failed with MMRESULT {result}."))
    }
}

const MMSYSERR_NOERROR: u32 = 0;

#[repr(C)]
#[derive(Default)]
struct TimeCaps {
    period_min: u32,
    period_max: u32,
}

#[link(name = "winmm")]
unsafe extern "system" {
    #[link_name = "timeGetDevCaps"]
    fn time_get_dev_caps(ptc: *mut TimeCaps, cbtc: u32) -> u32;
    #[link_name = "timeBeginPeriod"]
    fn time_begin_period(u_period: u32) -> u32;
    #[link_name = "timeEndPeriod"]
    fn time_end_period(u_period: u32) -> u32;
}
