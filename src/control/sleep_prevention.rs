pub(crate) use crate::platform::windows::sleep_prevention::SleepPreventionError;
use crate::platform::windows::sleep_prevention::{
    RequestKind, SleepPreventionPlatform, WindowsSleepPreventionPlatform,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SleepPreventionMode {
    #[default]
    Off,
    System,
    SystemAndDisplay,
}

/// Accepted requests owned by this controller, not proof of effective inhibition.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SleepPreventionRequests {
    pub(crate) system: bool,
    pub(crate) display: bool,
}

struct ActiveRequest<H> {
    handle: H,
    display: bool,
}

pub(crate) struct SleepPreventionController<
    P: SleepPreventionPlatform = WindowsSleepPreventionPlatform,
> {
    platform: P,
    active: Option<ActiveRequest<P::Handle>>,
}

impl Default for SleepPreventionController {
    fn default() -> Self {
        Self::new(WindowsSleepPreventionPlatform)
    }
}

impl<P: SleepPreventionPlatform> SleepPreventionController<P> {
    pub(crate) fn new(platform: P) -> Self {
        Self {
            platform,
            active: None,
        }
    }

    pub(crate) fn requests(&self) -> SleepPreventionRequests {
        SleepPreventionRequests {
            system: self.active.is_some(),
            display: self.active.as_ref().is_some_and(|active| active.display),
        }
    }

    pub(crate) fn reconcile(
        &mut self,
        mode: SleepPreventionMode,
        reason: &str,
    ) -> Result<(), SleepPreventionError> {
        if mode == SleepPreventionMode::Off {
            return self.shutdown();
        }
        let mut active = match self.active.take() {
            Some(active) => active,
            None => {
                let handle = self.platform.create(reason)?;
                self.platform.set(&handle, RequestKind::System)?;
                ActiveRequest {
                    handle,
                    display: false,
                }
            }
        };
        // Keep the accepted system count across polls and display changes so
        // reconciliation cannot renew Windows-terminated system requests.
        let display = mode == SleepPreventionMode::SystemAndDisplay;
        let result = if display == active.display {
            Ok(())
        } else if display {
            self.platform.set(&active.handle, RequestKind::Display)
        } else {
            self.platform.clear(&active.handle, RequestKind::Display)
        };
        if result.is_ok() {
            active.display = display;
        }
        self.active = Some(active);
        result
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), SleepPreventionError> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };
        let display = if active.display {
            self.platform.clear(&active.handle, RequestKind::Display)
        } else {
            Ok(())
        };
        let system = self.platform.clear(&active.handle, RequestKind::System);
        // Closing our last handle also destroys the request object when Windows
        // has already terminated its counts or rejects an explicit clear.
        drop(active);
        display.and(system)
    }
}

impl<P: SleepPreventionPlatform> Drop for SleepPreventionController<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
#[path = "sleep_prevention/tests.rs"]
mod tests;
