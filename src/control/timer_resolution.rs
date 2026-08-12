use std::fmt;

pub(crate) use crate::platform::windows::timer_resolution::TimerResolutionInfo;
use crate::platform::windows::timer_resolution::{
    TimerResolutionPlatform, WindowsTimerResolutionPlatform,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimerResolutionTransition {
    pub(crate) released_100ns: Option<u32>,
    pub(crate) active_100ns: u32,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimerResolutionTransitionStage {
    ReleasePrevious,
    RequestDesired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimerResolutionTransitionError {
    pub(crate) stage: TimerResolutionTransitionStage,
    pub(crate) released_100ns: Option<u32>,
    pub(crate) active_100ns: Option<u32>,
    pub(crate) message: String,
}

impl fmt::Display for TimerResolutionTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) struct TimerResolutionController<
    P: TimerResolutionPlatform = WindowsTimerResolutionPlatform,
> {
    platform: P,
    active_request_100ns: Option<u32>,
}

impl Default for TimerResolutionController<WindowsTimerResolutionPlatform> {
    fn default() -> Self {
        Self::new(WindowsTimerResolutionPlatform)
    }
}

impl<P> TimerResolutionController<P>
where
    P: TimerResolutionPlatform,
{
    pub(crate) fn new(platform: P) -> Self {
        Self {
            platform,
            active_request_100ns: None,
        }
    }

    pub(crate) fn query(&self) -> Result<TimerResolutionInfo, String> {
        self.platform.query()
    }

    pub(crate) fn active_request_100ns(&self) -> Option<u32> {
        self.active_request_100ns
    }

    pub(crate) fn set_request(
        &mut self,
        desired_100ns: u32,
    ) -> Result<TimerResolutionTransition, TimerResolutionTransitionError> {
        if self.active_request_100ns == Some(desired_100ns) {
            return Ok(TimerResolutionTransition {
                released_100ns: None,
                active_100ns: desired_100ns,
                changed: false,
            });
        }

        let previous_100ns = self.active_request_100ns;
        if let Some(previous_100ns) = previous_100ns {
            if let Err(message) = self.platform.release(previous_100ns) {
                return Err(TimerResolutionTransitionError {
                    stage: TimerResolutionTransitionStage::ReleasePrevious,
                    released_100ns: None,
                    active_100ns: Some(previous_100ns),
                    message,
                });
            }
            self.active_request_100ns = None;
        }

        match self.platform.request(desired_100ns) {
            Ok(active_100ns) => {
                self.active_request_100ns = Some(active_100ns);
                Ok(TimerResolutionTransition {
                    released_100ns: previous_100ns,
                    active_100ns,
                    changed: true,
                })
            }
            Err(message) => Err(TimerResolutionTransitionError {
                stage: TimerResolutionTransitionStage::RequestDesired,
                released_100ns: previous_100ns,
                active_100ns: None,
                message,
            }),
        }
    }

    pub(crate) fn release(&mut self) -> Result<Option<u32>, String> {
        let Some(active_100ns) = self.active_request_100ns else {
            return Ok(None);
        };
        self.platform.release(active_100ns)?;
        self.active_request_100ns = None;
        Ok(Some(active_100ns))
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.release().map(|_| ())
    }
}

impl<P> Drop for TimerResolutionController<P>
where
    P: TimerResolutionPlatform,
{
    fn drop(&mut self) {
        if let Some(active_100ns) = self.active_request_100ns.take() {
            let _ = self.platform.release(active_100ns);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    use super::*;

    #[derive(Default)]
    struct FakeState {
        requests: Vec<u32>,
        releases: Vec<u32>,
        request_results: VecDeque<Result<u32, String>>,
        release_results: VecDeque<Result<(), String>>,
    }

    #[derive(Clone)]
    struct FakePlatform(Rc<RefCell<FakeState>>);

    impl TimerResolutionPlatform for FakePlatform {
        fn query(&self) -> Result<TimerResolutionInfo, String> {
            Ok(TimerResolutionInfo {
                maximum_100ns: 160_000,
                minimum_100ns: 10_000,
            })
        }

        fn request(&self, desired_100ns: u32) -> Result<u32, String> {
            let mut state = self.0.borrow_mut();
            state.requests.push(desired_100ns);
            state
                .request_results
                .pop_front()
                .unwrap_or(Ok(desired_100ns))
        }

        fn release(&self, active_100ns: u32) -> Result<(), String> {
            let mut state = self.0.borrow_mut();
            state.releases.push(active_100ns);
            state.release_results.pop_front().unwrap_or(Ok(()))
        }
    }

    fn controller() -> (
        TimerResolutionController<FakePlatform>,
        Rc<RefCell<FakeState>>,
    ) {
        let state = Rc::new(RefCell::new(FakeState::default()));
        (
            TimerResolutionController::new(FakePlatform(Rc::clone(&state))),
            state,
        )
    }

    #[test]
    fn matching_request_is_not_duplicated() {
        let (mut controller, state) = controller();
        controller.set_request(10_000).unwrap();
        let unchanged = controller.set_request(10_000).unwrap();

        assert!(!unchanged.changed);
        assert_eq!(state.borrow().requests, vec![10_000]);
        assert!(state.borrow().releases.is_empty());
    }

    #[test]
    fn changing_request_releases_the_previous_period_first() {
        let (mut controller, state) = controller();
        controller.set_request(10_000).unwrap();
        let changed = controller.set_request(20_000).unwrap();

        assert_eq!(changed.released_100ns, Some(10_000));
        assert_eq!(changed.active_100ns, 20_000);
        assert_eq!(state.borrow().requests, vec![10_000, 20_000]);
        assert_eq!(state.borrow().releases, vec![10_000]);
    }

    #[test]
    fn failed_previous_release_keeps_the_owned_request() {
        let (mut controller, state) = controller();
        controller.set_request(10_000).unwrap();
        state
            .borrow_mut()
            .release_results
            .push_back(Err("release failed".to_owned()));

        let error = controller.set_request(20_000).unwrap_err();

        assert_eq!(error.stage, TimerResolutionTransitionStage::ReleasePrevious);
        assert_eq!(error.active_100ns, Some(10_000));
        assert_eq!(controller.active_request_100ns(), Some(10_000));
        assert_eq!(state.borrow().requests, vec![10_000]);
    }

    #[test]
    fn failed_new_request_leaves_no_active_request_after_release() {
        let (mut controller, state) = controller();
        controller.set_request(10_000).unwrap();
        state
            .borrow_mut()
            .request_results
            .push_back(Err("request failed".to_owned()));

        let error = controller.set_request(20_000).unwrap_err();

        assert_eq!(error.stage, TimerResolutionTransitionStage::RequestDesired);
        assert_eq!(error.released_100ns, Some(10_000));
        assert_eq!(error.active_100ns, None);
        assert_eq!(controller.active_request_100ns(), None);
    }

    #[test]
    fn shutdown_is_idempotent() {
        let (mut controller, state) = controller();
        controller.set_request(10_000).unwrap();

        controller.shutdown().unwrap();
        controller.shutdown().unwrap();

        assert_eq!(state.borrow().releases, vec![10_000]);
    }
}
