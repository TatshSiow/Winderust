use std::time::{Duration, Instant};

use crate::{
    backend::crash_recovery::{record_power_plan_change, RecoveryIntent},
    power::{
        active_plan, adaptive_power_profile_transition, apply_processor_power_values,
        create_adaptive_plan, delete_plan, set_active, AdaptivePowerProfile,
        ProcessorPowerAcDcValues, ProcessorPowerValues,
    },
    rules::{DecisionOutcome, DecisionState, ExecutionFailureTracker},
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PowerPlanOwner {
    OrdinaryAutomation,
    AdaptiveEngine,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PowerPlanStatus {
    pub(crate) owner: Option<PowerPlanOwner>,
    pub(crate) current_guid: Option<String>,
    pub(crate) target_guid: Option<String>,
    pub(crate) decision_state: Option<DecisionState>,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AdaptivePowerPlanRequest {
    pub(crate) profile: AdaptivePowerProfile,
    pub(crate) baseline: ProcessorPowerValues,
    pub(crate) has_efficiency_cores: bool,
}

struct ActiveAdaptivePowerPlan {
    original_guid: String,
    plan_guid: String,
    profile: AdaptivePowerProfile,
    baseline: ProcessorPowerValues,
    has_efficiency_cores: bool,
    lower_demand_since: Option<Instant>,
    active: bool,
}

pub(crate) trait PowerPlanRecoveryIntent {
    fn commit(self) -> Result<(), String>;
}

impl PowerPlanRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }
}

pub(crate) trait PowerPlanPlatform {
    type RecoveryIntent: PowerPlanRecoveryIntent;

    fn active_guid(&mut self) -> Result<String, String>;
    fn begin_switch(
        &mut self,
        original_guid: &str,
        expected_guid: &str,
    ) -> Result<Self::RecoveryIntent, String>;
    fn set_active(&mut self, guid: &str) -> Result<(), String>;
    fn create_adaptive_plan(&mut self, source_guid: &str) -> Result<String, String>;
    fn delete_plan(&mut self, guid: &str) -> Result<(), String>;
    fn apply_processor_values(
        &mut self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> Result<(), String>;
}

#[derive(Default)]
pub(crate) struct WindowsPowerPlanPlatform;

impl PowerPlanPlatform for WindowsPowerPlanPlatform {
    type RecoveryIntent = RecoveryIntent;

    fn active_guid(&mut self) -> Result<String, String> {
        active_plan().map(|plan| plan.guid)
    }

    fn begin_switch(
        &mut self,
        original_guid: &str,
        expected_guid: &str,
    ) -> Result<Self::RecoveryIntent, String> {
        record_power_plan_change(original_guid, expected_guid)
    }

    fn set_active(&mut self, guid: &str) -> Result<(), String> {
        set_active(guid)
    }

    fn create_adaptive_plan(&mut self, source_guid: &str) -> Result<String, String> {
        create_adaptive_plan(source_guid)
    }

    fn delete_plan(&mut self, guid: &str) -> Result<(), String> {
        delete_plan(guid)
    }

    fn apply_processor_values(
        &mut self,
        guid: &str,
        values: ProcessorPowerAcDcValues,
    ) -> Result<(), String> {
        apply_processor_power_values(guid, values)
    }
}

pub(crate) struct PowerPlanController<P: PowerPlanPlatform = WindowsPowerPlanPlatform> {
    platform: P,
    current_guid: Option<String>,
    last_verified_application: Option<String>,
    ordinary_original_guid: Option<String>,
    ordinary_expected_guid: Option<String>,
    adaptive_plan: Option<ActiveAdaptivePowerPlan>,
    last_decision: Option<DecisionOutcome>,
    next_active_plan_refresh: Option<Instant>,
    last_switch_attempt: Option<(String, Instant)>,
    switch_failures: ExecutionFailureTracker,
}

impl Default for PowerPlanController {
    fn default() -> Self {
        Self::with_platform(WindowsPowerPlanPlatform)
    }
}

impl<P: PowerPlanPlatform> PowerPlanController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            current_guid: None,
            last_verified_application: None,
            ordinary_original_guid: None,
            ordinary_expected_guid: None,
            adaptive_plan: None,
            last_decision: None,
            next_active_plan_refresh: None,
            last_switch_attempt: None,
            switch_failures: ExecutionFailureTracker::default(),
        }
    }

    pub(crate) fn adaptive_active(&self) -> bool {
        self.adaptive_plan.as_ref().is_some_and(|plan| plan.active)
    }

    pub(crate) fn clear_failures(&mut self) {
        self.switch_failures.clear();
    }

    pub(crate) fn status(&self) -> PowerPlanStatus {
        let adaptive_target = self
            .adaptive_plan
            .as_ref()
            .filter(|plan| plan.active)
            .map(|plan| plan.plan_guid.clone());
        let decision_target = self
            .last_decision
            .as_ref()
            .and_then(|decision| decision.power_plan_guid.clone());
        let owner = if adaptive_target.is_some() {
            Some(PowerPlanOwner::AdaptiveEngine)
        } else if decision_target.is_some() {
            Some(PowerPlanOwner::OrdinaryAutomation)
        } else {
            None
        };

        PowerPlanStatus {
            owner,
            current_guid: self.current_guid.clone(),
            target_guid: adaptive_target.or(decision_target),
            decision_state: self.last_decision.as_ref().map(|decision| decision.state),
            reason: self
                .last_decision
                .as_ref()
                .map(|decision| decision.reason.clone()),
        }
    }

    pub(crate) fn refresh_active_plan(&mut self, now: Instant) -> Result<(), String> {
        self.next_active_plan_refresh = Some(now + ACTIVE_PLAN_REFRESH_INTERVAL);
        let actual_guid = self.platform.active_guid()?;
        let external_break = self
            .last_verified_application
            .as_deref()
            .is_some_and(|expected| !same_guid(expected, &actual_guid));
        self.current_guid = Some(actual_guid.clone());

        if !external_break {
            return Ok(());
        }

        self.last_verified_application = None;
        self.ordinary_original_guid = None;
        self.ordinary_expected_guid = None;
        self.last_switch_attempt = None;

        if let Some(plan) = self.adaptive_plan.as_mut().filter(|plan| plan.active) {
            plan.active = false;
        }
        self.cleanup_inactive_adaptive_plan()
    }

    pub(crate) fn reconcile_ordinary(
        &mut self,
        decision: DecisionOutcome,
        now: Instant,
    ) -> Result<bool, String> {
        self.last_decision = Some(decision);
        if self.adaptive_active() {
            return Ok(false);
        }
        self.refresh_active_plan_if_due(now)?;

        let Some(target_guid) = self
            .last_decision
            .as_ref()
            .and_then(|decision| decision.power_plan_guid.clone())
        else {
            return Ok(false);
        };
        if self
            .current_guid
            .as_deref()
            .is_some_and(|current| same_guid(current, &target_guid))
        {
            self.clear_switch_failure(&target_guid);
            return Ok(false);
        }
        if self.is_switch_suppressed(&target_guid)
            || self.last_switch_attempt.as_ref().is_some_and(|(guid, at)| {
                same_guid(guid, &target_guid)
                    && now.saturating_duration_since(*at) < SWITCH_RETRY_INTERVAL
            })
        {
            return Ok(false);
        }

        self.last_switch_attempt = Some((target_guid.clone(), now));
        match switch_active_verified(&mut self.platform, &target_guid) {
            Ok(transition) => {
                if self
                    .last_verified_application
                    .as_deref()
                    .is_some_and(|expected| !same_guid(expected, &transition.previous_guid))
                {
                    self.clear_ordinary_ownership();
                }
                if self.ordinary_original_guid.is_none() {
                    self.ordinary_original_guid = Some(transition.previous_guid);
                }
                self.ordinary_expected_guid = Some(target_guid.clone());
                self.current_guid = Some(target_guid.clone());
                self.last_verified_application = Some(target_guid.clone());
                self.clear_switch_failure(&target_guid);
                if self
                    .ordinary_original_guid
                    .as_deref()
                    .is_some_and(|original| same_guid(original, &target_guid))
                {
                    self.clear_ordinary_ownership();
                }
                Ok(true)
            }
            Err(error) => {
                self.record_switch_failure(&target_guid);
                Err(error)
            }
        }
    }

    pub(crate) fn reconcile_adaptive(
        &mut self,
        request: AdaptivePowerPlanRequest,
        now: Instant,
    ) -> Result<AdaptivePowerProfile, String> {
        self.refresh_active_plan_if_due(now)?;
        self.cleanup_inactive_adaptive_plan()?;

        if self.adaptive_plan.is_none() {
            let original_guid = self.platform.active_guid()?;
            if self
                .last_verified_application
                .as_deref()
                .is_some_and(|expected| !same_guid(expected, &original_guid))
            {
                self.clear_ordinary_ownership();
                self.last_verified_application = None;
            }
            self.current_guid = Some(original_guid.clone());
            let plan_guid = self.platform.create_adaptive_plan(&original_guid)?;
            let values = request
                .profile
                .calibrated_power_values(request.baseline, request.has_efficiency_cores);
            if let Err(error) = self
                .platform
                .apply_processor_values(&plan_guid, values)
                .and_then(|()| switch_active_verified(&mut self.platform, &plan_guid).map(|_| ()))
            {
                return Err(adaptive_plan_setup_error(
                    error,
                    self.platform.delete_plan(&plan_guid),
                ));
            }
            self.current_guid = Some(plan_guid.clone());
            self.last_verified_application = Some(plan_guid.clone());
            self.adaptive_plan = Some(ActiveAdaptivePowerPlan {
                original_guid,
                plan_guid,
                profile: request.profile,
                baseline: request.baseline,
                has_efficiency_cores: request.has_efficiency_cores,
                lower_demand_since: None,
                active: true,
            });
        }

        let plan = self
            .adaptive_plan
            .as_mut()
            .ok_or_else(|| "Adaptive power plan was not initialized.".to_owned())?;
        let lower_demand_elapsed = if request.profile < plan.profile {
            now.saturating_duration_since(*plan.lower_demand_since.get_or_insert(now))
        } else {
            plan.lower_demand_since = None;
            Duration::ZERO
        };
        let next_profile =
            adaptive_power_profile_transition(plan.profile, request.profile, lower_demand_elapsed);
        if next_profile != plan.profile || request.baseline != plan.baseline {
            self.platform.apply_processor_values(
                &plan.plan_guid,
                next_profile.calibrated_power_values(request.baseline, plan.has_efficiency_cores),
            )?;
            plan.profile = next_profile;
            plan.baseline = request.baseline;
            plan.lower_demand_since = None;
        }

        Ok(plan.profile)
    }

    pub(crate) fn release_adaptive(&mut self, now: Instant) -> Result<(), String> {
        self.refresh_active_plan(now)?;
        let Some(mut plan) = self.adaptive_plan.take() else {
            return Ok(());
        };

        if plan.active {
            let current_matches = self
                .current_guid
                .as_deref()
                .is_some_and(|current| same_guid(current, &plan.plan_guid));
            if current_matches {
                match switch_active_verified(&mut self.platform, &plan.original_guid) {
                    Ok(_) => {
                        self.current_guid = Some(plan.original_guid.clone());
                        self.last_verified_application = self.ordinary_expected_guid.clone();
                        plan.active = false;
                    }
                    Err(error) => {
                        self.adaptive_plan = Some(plan);
                        return Err(error);
                    }
                }
            } else {
                self.clear_ordinary_ownership();
                self.last_verified_application = None;
                plan.active = false;
            }
        }

        if let Err(error) = self.platform.delete_plan(&plan.plan_guid) {
            self.adaptive_plan = Some(plan);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn shutdown(&mut self, now: Instant) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = self.release_adaptive(now) {
            errors.push(format!("restore adaptive power plan: {error}"));
        }
        if let Err(error) = self.restore_ordinary(now) {
            errors.push(format!("restore original power plan: {error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn restore_ordinary(&mut self, now: Instant) -> Result<(), String> {
        self.refresh_active_plan(now)?;
        let (Some(original_guid), Some(expected_guid)) = (
            self.ordinary_original_guid.clone(),
            self.ordinary_expected_guid.clone(),
        ) else {
            self.clear_ordinary_ownership();
            return Ok(());
        };
        let current_matches = self
            .current_guid
            .as_deref()
            .is_some_and(|current| same_guid(current, &expected_guid));
        if !current_matches {
            self.clear_ordinary_ownership();
            return Ok(());
        }

        match switch_active_verified(&mut self.platform, &original_guid) {
            Ok(_) => {
                self.current_guid = Some(original_guid);
                self.last_verified_application = None;
                self.clear_ordinary_ownership();
                Ok(())
            }
            Err(error) => Err(format!(
                "Cannot restore original power plan {original_guid}: {error}"
            )),
        }
    }

    fn refresh_active_plan_if_due(&mut self, now: Instant) -> Result<(), String> {
        if self
            .next_active_plan_refresh
            .is_none_or(|refresh_at| now >= refresh_at)
        {
            self.refresh_active_plan(now)
        } else {
            Ok(())
        }
    }

    fn cleanup_inactive_adaptive_plan(&mut self) -> Result<(), String> {
        let Some(plan) = self.adaptive_plan.as_ref().filter(|plan| !plan.active) else {
            return Ok(());
        };
        let guid = plan.plan_guid.clone();
        self.platform.delete_plan(&guid)?;
        self.adaptive_plan = None;
        Ok(())
    }

    fn clear_ordinary_ownership(&mut self) {
        self.ordinary_original_guid = None;
        self.ordinary_expected_guid = None;
    }

    fn is_switch_suppressed(&self, target_guid: &str) -> bool {
        self.switch_failures
            .is_key_suppressed(&switch_failure_key(target_guid))
    }

    fn record_switch_failure(&mut self, target_guid: &str) {
        self.switch_failures
            .record_key_failure(&switch_failure_key(target_guid));
    }

    fn clear_switch_failure(&mut self, target_guid: &str) {
        self.switch_failures
            .clear_key_failure(&switch_failure_key(target_guid));
    }
}

struct SwitchTransition {
    previous_guid: String,
}

fn switch_active_verified<P: PowerPlanPlatform>(
    platform: &mut P,
    target_guid: &str,
) -> Result<SwitchTransition, String> {
    let previous_guid = platform.active_guid()?;
    if same_guid(&previous_guid, target_guid) {
        return Ok(SwitchTransition { previous_guid });
    }

    let recovery = platform.begin_switch(&previous_guid, target_guid)?;
    if let Err(error) = platform.set_active(target_guid) {
        return compensate_failed_switch(
            platform,
            &previous_guid,
            recovery,
            format!("Power plan application failed: {error}"),
        );
    }
    match platform.active_guid() {
        Ok(actual_guid) if same_guid(&actual_guid, target_guid) => {
            if let Err(commit_error) = recovery.commit() {
                let compensation = platform.set_active(&previous_guid);
                return Err(compensated_switch_error(
                    format!("Power-plan recovery commit failed: {commit_error}"),
                    compensation,
                ));
            }
            Ok(SwitchTransition { previous_guid })
        }
        verification => {
            let verification_error = match verification {
                Ok(actual_guid) => format!(
                    "Power plan verification expected {target_guid}, but Windows reported {actual_guid}."
                ),
                Err(error) => format!("Power plan verification failed: {error}"),
            };
            compensate_failed_switch(platform, &previous_guid, recovery, verification_error)
        }
    }
}

fn compensate_failed_switch<P: PowerPlanPlatform>(
    platform: &mut P,
    previous_guid: &str,
    recovery: P::RecoveryIntent,
    operation_error: String,
) -> Result<SwitchTransition, String> {
    let compensation = platform.set_active(previous_guid);
    if compensation.is_ok() {
        drop(recovery);
        return Err(operation_error);
    }

    let commit_error = recovery.commit().err();
    let mut error = compensated_switch_error(operation_error, compensation);
    if let Some(commit_error) = commit_error {
        error.push_str(&format!(" Recovery commit also failed: {commit_error}"));
    }
    Err(error)
}

fn same_guid(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn switch_failure_key(target_guid: &str) -> String {
    target_guid.trim().to_ascii_lowercase()
}

fn compensated_switch_error(operation_error: String, compensation: Result<(), String>) -> String {
    match compensation {
        Ok(()) => operation_error,
        Err(compensation_error) => {
            format!(
                "{operation_error} Restoring the previous plan also failed: {compensation_error}"
            )
        }
    }
}

fn adaptive_plan_setup_error(operation_error: String, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => operation_error,
        Err(cleanup_error) => {
            format!("{operation_error} Adaptive plan cleanup also failed: {cleanup_error}")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

    use super::*;
    use crate::rules::DecisionState;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FailurePoint {
        Begin,
        Apply,
        Verify,
        Commit,
        Delete,
    }

    struct FakeRecoveryIntent {
        events: Rc<RefCell<Vec<String>>>,
        fail_commit: bool,
        completed: bool,
    }

    impl PowerPlanRecoveryIntent for FakeRecoveryIntent {
        fn commit(mut self) -> Result<(), String> {
            self.events.borrow_mut().push("commit".to_owned());
            self.completed = true;
            if self.fail_commit {
                Err("injected commit failure".to_owned())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for FakeRecoveryIntent {
        fn drop(&mut self) {
            if !self.completed {
                self.events.borrow_mut().push("cancel".to_owned());
            }
        }
    }

    struct FakePlatform {
        active: String,
        plans: BTreeSet<String>,
        events: Rc<RefCell<Vec<String>>>,
        failure: Option<FailurePoint>,
        verify_wrong_guid: bool,
        next_adaptive: u32,
    }

    impl FakePlatform {
        fn new(active: &str) -> Self {
            Self {
                active: active.to_owned(),
                plans: BTreeSet::from([active.to_owned()]),
                events: Rc::new(RefCell::new(Vec::new())),
                failure: None,
                verify_wrong_guid: false,
                next_adaptive: 1,
            }
        }
    }

    impl PowerPlanPlatform for FakePlatform {
        type RecoveryIntent = FakeRecoveryIntent;

        fn active_guid(&mut self) -> Result<String, String> {
            self.events.borrow_mut().push("query".to_owned());
            if self.failure == Some(FailurePoint::Verify) && self.verify_wrong_guid {
                self.verify_wrong_guid = false;
                return Ok("external".to_owned());
            }
            Ok(self.active.clone())
        }

        fn begin_switch(
            &mut self,
            original_guid: &str,
            expected_guid: &str,
        ) -> Result<Self::RecoveryIntent, String> {
            self.events
                .borrow_mut()
                .push(format!("begin:{original_guid}->{expected_guid}"));
            if self.failure == Some(FailurePoint::Begin) {
                return Err("injected begin failure".to_owned());
            }
            Ok(FakeRecoveryIntent {
                events: Rc::clone(&self.events),
                fail_commit: self.failure == Some(FailurePoint::Commit),
                completed: false,
            })
        }

        fn set_active(&mut self, guid: &str) -> Result<(), String> {
            self.events.borrow_mut().push(format!("apply:{guid}"));
            if self.failure == Some(FailurePoint::Apply) {
                self.failure = None;
                return Err("injected apply failure".to_owned());
            }
            self.active = guid.to_owned();
            if self.failure == Some(FailurePoint::Verify) {
                self.verify_wrong_guid = true;
            }
            Ok(())
        }

        fn create_adaptive_plan(&mut self, _source_guid: &str) -> Result<String, String> {
            let guid = format!("adaptive-{}", self.next_adaptive);
            self.next_adaptive += 1;
            self.plans.insert(guid.clone());
            Ok(guid)
        }

        fn delete_plan(&mut self, guid: &str) -> Result<(), String> {
            self.events.borrow_mut().push(format!("delete:{guid}"));
            if self.failure == Some(FailurePoint::Delete) {
                return Err("injected delete failure".to_owned());
            }
            self.plans.remove(guid);
            Ok(())
        }

        fn apply_processor_values(
            &mut self,
            guid: &str,
            _values: ProcessorPowerAcDcValues,
        ) -> Result<(), String> {
            self.events.borrow_mut().push(format!("configure:{guid}"));
            Ok(())
        }
    }

    fn decision(target: Option<&str>) -> DecisionOutcome {
        DecisionOutcome {
            power_plan_guid: target.map(str::to_owned),
            state: DecisionState::NoPowerPlanSelected,
            reason: "test decision".to_owned(),
        }
    }

    fn adaptive_request() -> AdaptivePowerPlanRequest {
        AdaptivePowerPlanRequest {
            profile: AdaptivePowerProfile::Responsive,
            baseline: ProcessorPowerValues::for_preset(
                crate::power::ProcessorPowerPreset::Balanced,
            ),
            has_efficiency_cores: false,
        }
    }

    #[test]
    fn switch_orders_begin_apply_verify_and_commit() {
        let mut platform = FakePlatform::new("original");
        let events = Rc::clone(&platform.events);

        switch_active_verified(&mut platform, "target").unwrap();

        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "commit"
            ]
        );
    }

    #[test]
    fn verification_failure_compensates_then_cancels() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Verify);
        let events = Rc::clone(&platform.events);

        assert!(switch_active_verified(&mut platform, "target").is_err());

        assert_eq!(platform.active, "original");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "apply:original",
                "cancel"
            ]
        );
    }

    #[test]
    fn application_failure_compensates_before_canceling_recovery() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Apply);
        let events = Rc::clone(&platform.events);

        assert!(switch_active_verified(&mut platform, "target").is_err());

        assert_eq!(platform.active, "original");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "apply:original",
                "cancel"
            ]
        );
    }

    #[test]
    fn recovery_commit_failure_restores_the_previous_plan() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Commit);
        let events = Rc::clone(&platform.events);

        assert!(switch_active_verified(&mut platform, "target").is_err());

        assert_eq!(platform.active, "original");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "commit",
                "apply:original"
            ]
        );
    }

    #[test]
    fn ordinary_owner_captures_first_baseline_and_releases_it() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));

        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller
            .reconcile_ordinary(
                decision(Some("second")),
                now + SWITCH_RETRY_INTERVAL + Duration::from_millis(1),
            )
            .unwrap();
        controller
            .shutdown(now + SWITCH_RETRY_INTERVAL * 2)
            .unwrap();

        assert_eq!(controller.platform.active, "original");
        let events = controller.platform.events.borrow();
        assert!(events.iter().any(|event| event == "begin:original->first"));
        assert!(events.iter().any(|event| event == "begin:first->second"));
        assert!(events.iter().any(|event| event == "begin:second->original"));
    }

    #[test]
    fn ordinary_reconcile_reports_only_verified_switches() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));

        assert!(controller
            .reconcile_ordinary(decision(Some("target")), now)
            .unwrap());
        assert!(!controller
            .reconcile_ordinary(decision(Some("target")), now + Duration::from_secs(1))
            .unwrap());
    }

    #[test]
    fn external_change_breaks_the_old_restore_chain() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("managed")), now)
            .unwrap();
        controller.platform.active = "external".to_owned();

        controller
            .refresh_active_plan(now + Duration::from_secs(1))
            .unwrap();
        controller.shutdown(now + Duration::from_secs(2)).unwrap();

        assert_eq!(controller.platform.active, "external");
    }

    #[test]
    fn external_change_immediately_before_a_new_rule_target_rebases_clean_restore() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller.platform.active = "external".to_owned();

        controller
            .reconcile_ordinary(decision(Some("second")), now + Duration::from_secs(1))
            .unwrap();
        controller.shutdown(now + Duration::from_secs(2)).unwrap();

        assert_eq!(controller.platform.active, "external");
    }

    #[test]
    fn adaptive_setup_rebases_to_an_external_plan_before_taking_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller.platform.active = "external".to_owned();

        controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .unwrap();
        controller
            .release_adaptive(now + Duration::from_secs(2))
            .unwrap();
        controller.shutdown(now + Duration::from_secs(3)).unwrap();

        assert_eq!(controller.platform.active, "external");
    }

    #[test]
    fn adaptive_owner_restores_ordinary_target_before_original_baseline() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            controller.status().owner,
            Some(PowerPlanOwner::AdaptiveEngine)
        );

        controller
            .release_adaptive(now + Duration::from_secs(2))
            .unwrap();
        assert_eq!(controller.platform.active, "ordinary");
        controller.shutdown(now + Duration::from_secs(3)).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn failed_adaptive_cleanup_remains_retryable() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Delete);

        assert!(controller
            .release_adaptive(now + Duration::from_secs(1))
            .is_err());
        assert!(controller.adaptive_plan.is_some());
        controller.platform.failure = None;
        controller
            .release_adaptive(now + Duration::from_secs(2))
            .unwrap();
        assert!(controller.adaptive_plan.is_none());
    }

    #[test]
    fn begin_failure_blocks_power_plan_mutation() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Begin);

        assert!(switch_active_verified(&mut platform, "target").is_err());
        assert_eq!(platform.active, "original");
        assert!(!platform
            .events
            .borrow()
            .iter()
            .any(|event| event == "apply:target"));
    }

    #[test]
    fn repeated_switch_failures_are_suppressed_case_insensitively() {
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));

        controller.record_switch_failure("PLAN-GUID");
        controller.record_switch_failure("plan-guid");
        assert!(!controller.is_switch_suppressed("plan-guid"));

        controller.record_switch_failure("plan-guid");
        assert!(controller.is_switch_suppressed("plan-guid"));

        controller.clear_switch_failure("PLAN-GUID");
        assert!(!controller.is_switch_suppressed("plan-guid"));
    }

    #[test]
    fn adaptive_setup_error_preserves_cleanup_failure() {
        assert_eq!(
            adaptive_plan_setup_error(
                "Applying the adaptive plan failed.".to_owned(),
                Err("Deleting the adaptive plan failed.".to_owned()),
            ),
            "Applying the adaptive plan failed. Adaptive plan cleanup also failed: Deleting the adaptive plan failed."
        );
    }
}
