use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum RefreshDomain {
    PowerPlanCheck,
    BackgroundEfficiency,
    AppSuspension,
    AppSuspensionForegroundRelease,
    CpuSetsSoft,
    ProcessorAffinityHard,
    CoreLimiter,
    CpuAllocationReconciliation,
    ByRunningApp,
    WorkloadEngine,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    MemoryTrim,
    TimerResolution,
    ProcessAppearance,
    ControllerActivity,
}

pub(crate) const ALL_REFRESH_DOMAINS: [RefreshDomain; 20] = [
    RefreshDomain::PowerPlanCheck,
    RefreshDomain::BackgroundEfficiency,
    RefreshDomain::AppSuspension,
    RefreshDomain::AppSuspensionForegroundRelease,
    RefreshDomain::CpuSetsSoft,
    RefreshDomain::ProcessorAffinityHard,
    RefreshDomain::CoreLimiter,
    RefreshDomain::CpuAllocationReconciliation,
    RefreshDomain::ByRunningApp,
    RefreshDomain::WorkloadEngine,
    RefreshDomain::ProcessPriority,
    RefreshDomain::ThreadPriority,
    RefreshDomain::DynamicPriorityBoost,
    RefreshDomain::IoPriority,
    RefreshDomain::GpuPriority,
    RefreshDomain::MemoryPriority,
    RefreshDomain::MemoryTrim,
    RefreshDomain::TimerResolution,
    RefreshDomain::ProcessAppearance,
    RefreshDomain::ControllerActivity,
];

const FOREGROUND_DOMAINS: [RefreshDomain; 15] = [
    RefreshDomain::PowerPlanCheck,
    RefreshDomain::BackgroundEfficiency,
    RefreshDomain::AppSuspensionForegroundRelease,
    RefreshDomain::CpuSetsSoft,
    RefreshDomain::ProcessorAffinityHard,
    RefreshDomain::CoreLimiter,
    RefreshDomain::WorkloadEngine,
    RefreshDomain::ProcessPriority,
    RefreshDomain::ThreadPriority,
    RefreshDomain::DynamicPriorityBoost,
    RefreshDomain::IoPriority,
    RefreshDomain::GpuPriority,
    RefreshDomain::MemoryPriority,
    RefreshDomain::MemoryTrim,
    RefreshDomain::TimerResolution,
];

const WINDOW_CREATED_DOMAINS: [RefreshDomain; 2] = [
    RefreshDomain::ProcessAppearance,
    RefreshDomain::AppSuspension,
];

const APP_SWITCH_DOMAINS: [RefreshDomain; 2] = [
    RefreshDomain::AppSuspensionForegroundRelease,
    RefreshDomain::TimerResolution,
];

const PROCESS_APPEARANCE_DOMAINS: [RefreshDomain; 13] = [
    RefreshDomain::BackgroundEfficiency,
    RefreshDomain::CpuSetsSoft,
    RefreshDomain::ProcessorAffinityHard,
    RefreshDomain::CoreLimiter,
    RefreshDomain::ByRunningApp,
    RefreshDomain::WorkloadEngine,
    RefreshDomain::ProcessPriority,
    RefreshDomain::ThreadPriority,
    RefreshDomain::DynamicPriorityBoost,
    RefreshDomain::IoPriority,
    RefreshDomain::GpuPriority,
    RefreshDomain::MemoryPriority,
    RefreshDomain::MemoryTrim,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulerEvent {
    SettingsChanged,
    ForegroundChanged,
    WindowCreated,
    PowerChanged,
    SessionChanged,
    InputActivity,
    AppSwitch,
    AppSwitchMouseClick,
    ProcessAppeared,
    AppSuspensionRequested,
    MemoryTrimRequested,
    ControllerActivity,
}

/// Owns the outer runtime worker deadlines. Feature-local sampling history and cooldowns remain
/// with their existing managers.
pub(crate) struct RefreshScheduler {
    deadlines: [Instant; ALL_REFRESH_DOMAINS.len()],
    workload_engine_fast_until: Option<Instant>,
}

impl RefreshScheduler {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            deadlines: [now; ALL_REFRESH_DOMAINS.len()],
            workload_engine_fast_until: None,
        }
    }

    pub(crate) fn is_due(&self, domain: RefreshDomain, now: Instant) -> bool {
        now >= self.deadline(domain)
    }

    pub(crate) fn schedule_now(&mut self, domain: RefreshDomain, now: Instant) {
        self.deadlines[domain as usize] = now;
    }

    pub(crate) fn schedule_after(
        &mut self,
        domain: RefreshDomain,
        now: Instant,
        interval: Duration,
    ) {
        self.deadlines[domain as usize] = now + interval;
    }

    pub(crate) fn invalidate(
        &mut self,
        event: SchedulerEvent,
        now: Instant,
        workload_fast_refresh_enabled: bool,
        workload_fast_window: Duration,
    ) {
        match event {
            SchedulerEvent::SettingsChanged => {
                self.schedule_domains_now(&ALL_REFRESH_DOMAINS, now);
                self.workload_engine_fast_until = None;
            }
            SchedulerEvent::ForegroundChanged => {
                self.schedule_domains_now(&FOREGROUND_DOMAINS, now);
                self.extend_workload_fast_window(
                    now,
                    workload_fast_refresh_enabled,
                    workload_fast_window,
                );
            }
            SchedulerEvent::WindowCreated => {
                self.schedule_domains_now(&WINDOW_CREATED_DOMAINS, now);
                self.extend_workload_fast_window(
                    now,
                    workload_fast_refresh_enabled,
                    workload_fast_window,
                );
            }
            SchedulerEvent::PowerChanged | SchedulerEvent::InputActivity => {
                self.schedule_now(RefreshDomain::PowerPlanCheck, now);
            }
            SchedulerEvent::SessionChanged => {
                self.schedule_domains_now(&FOREGROUND_DOMAINS, now);
                self.schedule_domains_now(&WINDOW_CREATED_DOMAINS, now);
                self.extend_workload_fast_window(
                    now,
                    workload_fast_refresh_enabled,
                    workload_fast_window,
                );
            }
            SchedulerEvent::AppSwitch | SchedulerEvent::AppSwitchMouseClick => {
                self.schedule_domains_now(&APP_SWITCH_DOMAINS, now);
            }
            SchedulerEvent::ProcessAppeared => {
                self.schedule_domains_now(&PROCESS_APPEARANCE_DOMAINS, now);
                self.extend_workload_fast_window(
                    now,
                    workload_fast_refresh_enabled,
                    workload_fast_window,
                );
            }
            SchedulerEvent::AppSuspensionRequested => {
                self.schedule_now(RefreshDomain::AppSuspension, now);
            }
            SchedulerEvent::MemoryTrimRequested => {
                self.schedule_now(RefreshDomain::MemoryTrim, now);
            }
            SchedulerEvent::ControllerActivity => {
                self.schedule_now(RefreshDomain::PowerPlanCheck, now);
            }
        }
    }

    pub(crate) fn extend_workload_fast_window(
        &mut self,
        now: Instant,
        enabled: bool,
        window: Duration,
    ) {
        if enabled {
            self.workload_engine_fast_until = Some(now + window);
        }
    }

    pub(crate) fn workload_fast_refresh_active(&self, now: Instant, enabled: bool) -> bool {
        enabled
            && self
                .workload_engine_fast_until
                .is_some_and(|until| now < until)
    }

    pub(crate) fn minimum_wait(
        &self,
        current: Option<Duration>,
        now: Instant,
        domains: impl IntoIterator<Item = (bool, RefreshDomain, Duration)>,
    ) -> Option<Duration> {
        domains
            .into_iter()
            .filter(|(required, _, _)| *required)
            .fold(current, |wait, (_, domain, interval)| {
                let candidate = self
                    .deadline(domain)
                    .saturating_duration_since(now)
                    .min(interval);
                Some(wait.map_or(candidate, |wait| wait.min(candidate)))
            })
    }

    fn deadline(&self, domain: RefreshDomain) -> Instant {
        self.deadlines[domain as usize]
    }

    fn schedule_domains_now(&mut self, domains: &[RefreshDomain], now: Instant) {
        for domain in domains {
            self.schedule_now(*domain, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(60 * 60);
    const FAST_WINDOW: Duration = Duration::from_secs(8);

    fn future_scheduler(now: Instant) -> RefreshScheduler {
        let mut scheduler = RefreshScheduler::new(now);
        for domain in ALL_REFRESH_DOMAINS {
            scheduler.schedule_after(domain, now, HOUR);
        }
        scheduler
    }

    fn assert_only_due(scheduler: &RefreshScheduler, now: Instant, expected: &[RefreshDomain]) {
        for domain in ALL_REFRESH_DOMAINS {
            assert_eq!(
                scheduler.is_due(domain, now),
                expected.contains(&domain),
                "unexpected due state for {domain:?}"
            );
        }
    }

    #[test]
    fn settings_invalidates_every_domain_and_resets_the_fast_window() {
        let now = Instant::now();
        let mut scheduler = future_scheduler(now);
        scheduler.extend_workload_fast_window(now, true, FAST_WINDOW);

        scheduler.invalidate(SchedulerEvent::SettingsChanged, now, true, FAST_WINDOW);

        assert_only_due(&scheduler, now, &ALL_REFRESH_DOMAINS);
        assert!(!scheduler.workload_fast_refresh_active(now, true));
    }

    #[test]
    fn foreground_invalidates_only_foreground_sensitive_domains() {
        let now = Instant::now();
        let mut scheduler = future_scheduler(now);

        scheduler.invalidate(SchedulerEvent::ForegroundChanged, now, true, FAST_WINDOW);

        assert_only_due(&scheduler, now, &FOREGROUND_DOMAINS);
        assert!(scheduler.workload_fast_refresh_active(now, true));
    }

    #[test]
    fn window_creation_invalidates_process_appearance_and_app_suspension() {
        let now = Instant::now();
        let mut scheduler = future_scheduler(now);

        scheduler.invalidate(SchedulerEvent::WindowCreated, now, false, FAST_WINDOW);

        assert_only_due(&scheduler, now, &WINDOW_CREATED_DOMAINS);
        assert!(!scheduler.workload_fast_refresh_active(now, true));
    }

    #[test]
    fn session_change_combines_foreground_window_and_power_invalidations() {
        let now = Instant::now();
        let mut scheduler = future_scheduler(now);
        let expected = ALL_REFRESH_DOMAINS
            .into_iter()
            .filter(|domain| {
                FOREGROUND_DOMAINS.contains(domain) || WINDOW_CREATED_DOMAINS.contains(domain)
            })
            .collect::<Vec<_>>();

        scheduler.invalidate(SchedulerEvent::SessionChanged, now, true, FAST_WINDOW);

        assert_only_due(&scheduler, now, &expected);
        assert!(scheduler.workload_fast_refresh_active(now, true));
    }

    #[test]
    fn power_input_and_controller_activity_only_force_a_power_plan_check() {
        let now = Instant::now();
        for event in [
            SchedulerEvent::PowerChanged,
            SchedulerEvent::InputActivity,
            SchedulerEvent::ControllerActivity,
        ] {
            let mut scheduler = future_scheduler(now);
            scheduler.invalidate(event, now, false, FAST_WINDOW);
            assert_only_due(&scheduler, now, &[RefreshDomain::PowerPlanCheck]);
        }
    }

    #[test]
    fn both_app_switch_inputs_share_the_same_deadline_mapping() {
        let now = Instant::now();
        for event in [
            SchedulerEvent::AppSwitch,
            SchedulerEvent::AppSwitchMouseClick,
        ] {
            let mut scheduler = future_scheduler(now);
            scheduler.invalidate(event, now, false, FAST_WINDOW);
            assert_only_due(&scheduler, now, &APP_SWITCH_DOMAINS);
        }
    }

    #[test]
    fn process_appearance_invalidates_only_process_consumers() {
        let now = Instant::now();
        let mut scheduler = future_scheduler(now);

        scheduler.invalidate(SchedulerEvent::ProcessAppeared, now, true, FAST_WINDOW);

        assert_only_due(&scheduler, now, &PROCESS_APPEARANCE_DOMAINS);
        assert!(scheduler.workload_fast_refresh_active(now, true));
    }

    #[test]
    fn manual_requests_force_only_the_requested_domain() {
        let now = Instant::now();
        let mut suspension = future_scheduler(now);
        suspension.invalidate(
            SchedulerEvent::AppSuspensionRequested,
            now,
            false,
            FAST_WINDOW,
        );
        assert_only_due(&suspension, now, &[RefreshDomain::AppSuspension]);

        let mut trim = future_scheduler(now);
        trim.invalidate(SchedulerEvent::MemoryTrimRequested, now, false, FAST_WINDOW);
        assert_only_due(&trim, now, &[RefreshDomain::MemoryTrim]);
    }

    #[test]
    fn fast_window_expires_at_its_deadline_and_disabled_features_do_not_extend_it() {
        let now = Instant::now();
        let mut scheduler = RefreshScheduler::new(now);
        scheduler.extend_workload_fast_window(now, false, FAST_WINDOW);
        assert!(!scheduler.workload_fast_refresh_active(now, true));

        scheduler.extend_workload_fast_window(now, true, FAST_WINDOW);
        assert!(scheduler.workload_fast_refresh_active(now, true));
        assert!(!scheduler.workload_fast_refresh_active(now + FAST_WINDOW, true));
        assert!(!scheduler.workload_fast_refresh_active(now, false));
    }

    #[test]
    fn minimum_wait_uses_only_required_domains_and_preserves_dormancy() {
        let now = Instant::now();
        let mut scheduler = RefreshScheduler::new(now);
        scheduler.schedule_after(
            RefreshDomain::BackgroundEfficiency,
            now,
            Duration::from_secs(9),
        );
        scheduler.schedule_after(RefreshDomain::MemoryTrim, now, Duration::from_secs(5));

        assert_eq!(scheduler.minimum_wait(None, now, []), None);
        assert_eq!(
            scheduler.minimum_wait(
                None,
                now,
                [
                    (
                        true,
                        RefreshDomain::BackgroundEfficiency,
                        Duration::from_secs(3),
                    ),
                    (false, RefreshDomain::MemoryTrim, Duration::from_secs(1),),
                ],
            ),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            scheduler.minimum_wait(
                Some(Duration::from_secs(2)),
                now,
                [(true, RefreshDomain::MemoryTrim, Duration::from_secs(10),)],
            ),
            Some(Duration::from_secs(2))
        );
    }
}
