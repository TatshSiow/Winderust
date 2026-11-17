use std::time::{Duration, Instant};

use crate::{
    backend::crash_recovery::{
        forget_power_plan_changes, record_power_plan_change, DeferredRecoveryIntent, RecoveryIntent,
    },
    power::{
        active_plan, adaptive_power_profile_transition, apply_processor_power_values, delete_plan,
        duplicate_adaptive_plan, initialize_adaptive_plan, set_active, AdaptivePowerBoostValues,
        AdaptivePowerProfile, ProcessorPowerSourceValues, ProcessorPowerValues,
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
    pub(crate) apply_failed: bool,
    pub(crate) rule_index: Option<usize>,
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
    pub(crate) background_pressure_profile: AdaptivePowerBoostValues,
    pub(crate) focus_and_launch_profile: AdaptivePowerBoostValues,
}

struct ActiveAdaptivePowerPlan {
    original_guid: String,
    plan_guid: String,
    profile: AdaptivePowerProfile,
    values: Option<ProcessorPowerSourceValues>,
    lower_demand_since: Option<Instant>,
    active: bool,
}

pub(crate) trait PowerPlanRecoveryIntent {
    type Deferred: PowerPlanDeferredRecoveryIntent;

    fn commit(self) -> Result<(), (String, Self::Deferred)>;
    fn defer(self) -> Self::Deferred;
}

pub(crate) trait PowerPlanDeferredRecoveryIntent {
    fn resolve(&mut self, applied: bool) -> Result<(), String>;
}

impl PowerPlanDeferredRecoveryIntent for DeferredRecoveryIntent {
    fn resolve(&mut self, applied: bool) -> Result<(), String> {
        DeferredRecoveryIntent::resolve(self, applied)
    }
}

impl PowerPlanRecoveryIntent for RecoveryIntent {
    type Deferred = DeferredRecoveryIntent;

    fn commit(self) -> Result<(), (String, Self::Deferred)> {
        RecoveryIntent::commit_or_defer(self)
    }

    fn defer(self) -> Self::Deferred {
        RecoveryIntent::defer(self)
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
    fn forget_switches(&mut self) -> Result<(), String>;
    fn set_active(&mut self, guid: &str) -> Result<(), String>;
    fn duplicate_adaptive_plan(&mut self, source_guid: &str) -> Result<String, String>;
    fn initialize_adaptive_plan(&mut self, guid: &str, source_guid: &str) -> Result<(), String>;
    fn delete_plan(&mut self, guid: &str) -> Result<(), String>;
    fn apply_processor_values(
        &mut self,
        guid: &str,
        values: ProcessorPowerSourceValues,
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

    fn forget_switches(&mut self) -> Result<(), String> {
        forget_power_plan_changes()
    }

    fn duplicate_adaptive_plan(&mut self, source_guid: &str) -> Result<String, String> {
        duplicate_adaptive_plan(source_guid)
    }

    fn initialize_adaptive_plan(&mut self, guid: &str, source_guid: &str) -> Result<(), String> {
        initialize_adaptive_plan(guid, source_guid)
    }

    fn delete_plan(&mut self, guid: &str) -> Result<(), String> {
        delete_plan(guid)
    }

    fn apply_processor_values(
        &mut self,
        guid: &str,
        values: ProcessorPowerSourceValues,
    ) -> Result<(), String> {
        apply_processor_power_values(guid, values)
    }
}

pub(crate) struct PowerPlanController<P: PowerPlanPlatform = WindowsPowerPlanPlatform> {
    platform: P,
    current_guid: Option<String>,
    last_verified_application: Option<String>,
    ownership_release_pending: bool,
    uncertain_switch:
        Option<UncertainSwitch<<P::RecoveryIntent as PowerPlanRecoveryIntent>::Deferred>>,
    ordinary_original_guid: Option<String>,
    ordinary_expected_guid: Option<String>,
    temporary_plan: Option<TemporaryPowerPlan>,
    adaptive_plan: Option<ActiveAdaptivePowerPlan>,
    setup_cleanup_due: Option<Instant>,
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

#[cfg(test)]
impl PowerPlanController {
    pub(crate) fn temporary_adaptive_fixture() -> Self {
        Self {
            temporary_plan: Some(TemporaryPowerPlan {
                original_guid: "fixture-original".into(),
                expected_guid: "fixture-selected".into(),
                owner: TemporaryOwner::Time,
            }),
            adaptive_plan: Some(ActiveAdaptivePowerPlan {
                original_guid: "fixture-selected".into(),
                plan_guid: "fixture-adaptive".into(),
                profile: AdaptivePowerProfile::Responsive,
                values: None,
                lower_demand_since: None,
                active: true,
            }),
            ..Self::default()
        }
    }
}

impl<P: PowerPlanPlatform> PowerPlanController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            current_guid: None,
            last_verified_application: None,
            ownership_release_pending: false,
            uncertain_switch: None,
            ordinary_original_guid: None,
            ordinary_expected_guid: None,
            temporary_plan: None,
            adaptive_plan: None,
            setup_cleanup_due: None,
            last_decision: None,
            next_active_plan_refresh: None,
            last_switch_attempt: None,
            switch_failures: ExecutionFailureTracker::default(),
        }
    }

    pub(crate) fn adaptive_active(&self) -> bool {
        self.adaptive_plan.as_ref().is_some_and(|plan| plan.active)
    }

    pub(crate) fn has_focus_plan(&self) -> bool {
        self.temporary_plan
            .as_ref()
            .is_some_and(|plan| matches!(plan.owner, TemporaryOwner::Focus(_)))
            || self.uncertain_switch.as_ref().is_some_and(|pending| {
                matches!(
                    pending.temporary_effect,
                    TemporaryEffect::Acquire(TemporaryOwner::Focus(_))
                )
            })
    }

    pub(crate) fn has_temporary_plan(&self) -> bool {
        self.temporary_plan.is_some()
            || self.uncertain_switch.as_ref().is_some_and(|pending| {
                matches!(
                    pending.temporary_effect,
                    TemporaryEffect::Acquire(_) | TemporaryEffect::Release
                )
            })
    }

    pub(crate) fn cancel_temporary_restoration(&mut self) {
        self.temporary_plan = None;
        if let Some(pending) = &mut self.uncertain_switch {
            // Cancel the optional return without discarding the recovery acknowledgement.
            pending.temporary_effect = TemporaryEffect::Release;
        }
    }

    pub(crate) fn temporary_release_retry_delay(&self) -> Option<Duration> {
        (self.has_temporary_plan()
            && (self.uncertain_switch.is_some()
                || self.next_active_plan_refresh.is_none()
                || self.last_decision.as_ref().is_some_and(|decision| {
                    !decision.restore_previous_plan
                        || self
                            .temporary_plan
                            .as_ref()
                            .is_some_and(|plan| plan.owner.observation_unavailable(decision))
                        || decision
                            .power_plan_guid
                            .as_deref()
                            .zip(self.current_guid.as_deref())
                            .is_none_or(|(target, current)| !same_guid(target, current))
                })))
        .then_some(Duration::from_secs(1))
    }

    pub(crate) fn cleanup_retry_delay(&self, now: Instant) -> Option<Duration> {
        self.setup_cleanup_due
            .map(|due| due.saturating_duration_since(now))
    }

    pub(crate) fn retry_pending_cleanup(&mut self, now: Instant) -> Result<(), String> {
        if self.cleanup_retry_delay(now) == Some(Duration::ZERO) {
            self.setup_cleanup_due = Some(now + SWITCH_RETRY_INTERVAL);
            self.release_adaptive(now)?;
        }
        Ok(())
    }

    pub(crate) fn has_pending_cleanup(&self) -> bool {
        self.adaptive_plan.is_some()
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
            apply_failed: decision_target.as_deref().is_some_and(|guid| {
                self.switch_failures
                    .has_key_failure(&switch_failure_key(guid))
            }),
            rule_index: self
                .last_decision
                .as_ref()
                .and_then(|decision| decision.rule_index),
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
        self.next_active_plan_refresh = None;
        let actual_guid = self.platform.active_guid()?;
        self.observe_active_plan(&actual_guid)?;
        self.next_active_plan_refresh = Some(now + ACTIVE_PLAN_REFRESH_INTERVAL);
        self.cleanup_inactive_adaptive_plan()
    }

    fn observe_active_plan(&mut self, actual_guid: &str) -> Result<(), String> {
        if self.ownership_release_pending {
            self.relinquish_ownership()?;
            self.current_guid = Some(actual_guid.to_owned());
            return Ok(());
        }
        if let Some(mut uncertain) = self.uncertain_switch.take() {
            let transition = &uncertain.transition;
            let at_target = same_guid(actual_guid, &transition.target_guid);
            let at_previous = same_guid(actual_guid, &transition.previous_guid);
            if !at_target && (!at_previous || !uncertain.allow_previous) {
                // Forget the whole chain in one acknowledged command. A failed
                // Commit was never compensated, so even its previous value now
                // represents a later external selection.
                self.uncertain_switch = Some(uncertain);
                self.relinquish_ownership()?;
                self.current_guid = Some(actual_guid.to_owned());
                return Ok(());
            }
            if let Err(error) = uncertain.recovery.resolve(at_target) {
                self.uncertain_switch = Some(uncertain);
                return Err(format!(
                    "Resolving the pending power-plan change failed: {error}"
                ));
            }
            if same_guid(actual_guid, &transition.target_guid) {
                let adaptive_transition = self.adaptive_plan.as_mut().is_some_and(|plan| {
                    if same_guid(&plan.plan_guid, actual_guid) {
                        plan.active = true;
                        true
                    } else if same_guid(&plan.plan_guid, &transition.previous_guid) {
                        plan.active = false;
                        true
                    } else {
                        false
                    }
                });
                if !adaptive_transition {
                    self.record_ordinary_switch(transition, uncertain.temporary_effect);
                }
                self.last_verified_application = Some(actual_guid.to_owned());
            }
        }
        let external_break = self.ownership_release_pending
            || self
                .last_verified_application
                .as_deref()
                .is_some_and(|expected| !same_guid(expected, actual_guid));
        if external_break {
            self.relinquish_ownership()?;
        }
        self.current_guid = Some(actual_guid.to_owned());
        Ok(())
    }

    fn relinquish_ownership(&mut self) -> Result<(), String> {
        // Keep the observed break latched until the helper acknowledges relinquishment.
        self.ownership_release_pending = true;
        self.platform.forget_switches()?;
        self.ownership_release_pending = false;
        self.uncertain_switch = None;
        self.temporary_plan = None;
        self.last_verified_application = None;
        self.clear_ordinary_ownership();
        self.last_switch_attempt = None;
        if let Some(plan) = self.adaptive_plan.as_mut().filter(|plan| plan.active) {
            plan.active = false;
        }
        Ok(())
    }

    fn switch_active_verified(
        &mut self,
        target_guid: &str,
        temporary_effect: TemporaryEffect,
    ) -> Result<SwitchTransition, String> {
        let previous_guid = self.platform.active_guid()?;
        self.observe_active_plan(&previous_guid)?;
        if let Some(plan) = self
            .adaptive_plan
            .as_mut()
            .filter(|plan| !plan.active && same_guid(&plan.plan_guid, target_guid))
        {
            plan.original_guid = previous_guid.clone();
        }
        self.switch_from_current(previous_guid, target_guid, temporary_effect)
    }

    fn switch_from_current(
        &mut self,
        previous_guid: String,
        target_guid: &str,
        temporary_effect: TemporaryEffect,
    ) -> Result<SwitchTransition, String> {
        match switch_from_verified(&mut self.platform, previous_guid.clone(), target_guid) {
            Ok(transition) => Ok(transition),
            Err(failure) => {
                let mut error = failure.message;
                match failure.state {
                    FailedSwitchState::Previous => self.current_guid = Some(previous_guid),
                    FailedSwitchState::External(actual_guid) => {
                        // The transaction has released its recovery intent (and journal lock).
                        if let Err(forget_error) = self.relinquish_ownership() {
                            error.push_str(&format!(
                                " Relinquishing power-plan ownership also failed: {forget_error}"
                            ));
                        } else {
                            self.current_guid = Some(actual_guid);
                        }
                    }
                    FailedSwitchState::Pending {
                        recovery,
                        allow_previous,
                    } => {
                        // A later observation must resolve our attempted mutation before it
                        // can be classified as an external change or a new baseline.
                        self.uncertain_switch = Some(UncertainSwitch {
                            transition: SwitchTransition {
                                previous_guid,
                                target_guid: target_guid.to_owned(),
                            },
                            recovery,
                            allow_previous,
                            temporary_effect,
                        });
                    }
                }
                Err(error)
            }
        }
    }

    fn restore_active_verified(
        &mut self,
        expected_guid: &str,
        original_guid: &str,
        temporary_effect: TemporaryEffect,
    ) -> Result<bool, String> {
        let actual_guid = self.platform.active_guid()?;
        if self.ownership_release_pending || !same_guid(&actual_guid, expected_guid) {
            self.relinquish_ownership()?;
            self.current_guid = Some(actual_guid);
            return Ok(false);
        }
        let transition = self.switch_from_current(actual_guid, original_guid, temporary_effect)?;
        if !matches!(temporary_effect, TemporaryEffect::Preserve) {
            self.record_ordinary_switch(&transition, temporary_effect);
        }
        Ok(true)
    }

    fn record_ordinary_switch(
        &mut self,
        transition: &SwitchTransition,
        temporary_effect: TemporaryEffect,
    ) {
        self.ordinary_original_guid
            .get_or_insert_with(|| transition.previous_guid.clone());
        self.ordinary_expected_guid = Some(transition.target_guid.clone());
        self.current_guid = Some(transition.target_guid.clone());
        self.last_verified_application = Some(transition.target_guid.clone());
        if self
            .ordinary_original_guid
            .as_deref()
            .is_some_and(|original| same_guid(original, &transition.target_guid))
        {
            self.clear_ordinary_ownership();
        }
        match temporary_effect {
            TemporaryEffect::Preserve => {}
            TemporaryEffect::Acquire(owner) => {
                let plan = self
                    .temporary_plan
                    .get_or_insert_with(|| TemporaryPowerPlan {
                        original_guid: transition.previous_guid.clone(),
                        expected_guid: transition.target_guid.clone(),
                        owner,
                    });
                plan.expected_guid.clone_from(&transition.target_guid);
                plan.owner = owner;
            }
            TemporaryEffect::Release => self.temporary_plan = None,
        }
    }

    pub(crate) fn reconcile_ordinary(
        &mut self,
        decision: DecisionOutcome,
        now: Instant,
    ) -> Result<bool, String> {
        if !decision.restore_previous_plan {
            self.cancel_temporary_restoration();
        }
        self.last_decision = Some(decision);
        if self.adaptive_active()
            && !self.ownership_release_pending
            && self.uncertain_switch.is_none()
        {
            return Ok(false);
        }
        self.refresh_active_plan_if_due(now)?;

        let decision = self
            .last_decision
            .as_ref()
            .ok_or_else(|| "Power-plan reconciliation lost its decision.".to_owned())?;
        let stop_requested = matches!(
            decision.state,
            DecisionState::Disabled | DecisionState::PausedWhilePluggedIn
        );
        if !stop_requested
            && self
                .temporary_plan
                .as_ref()
                .is_some_and(|plan| plan.owner.observation_unavailable(decision))
        {
            return Ok(false);
        }
        let Some(target_guid) = decision.power_plan_guid.clone() else {
            return self.restore_temporary_plan(now);
        };
        let temporary_effect = if decision.restore_previous_plan {
            TemporaryEffect::Acquire(TemporaryOwner::from_decision(decision)?)
        } else {
            TemporaryEffect::Release
        };
        if matches!(temporary_effect, TemporaryEffect::Acquire(_)) && self.temporary_plan.is_none()
        {
            // A cached match cannot establish the plan that preceded this episode.
            // Rebase external changes before deciding whether acquisition needs a switch.
            self.refresh_active_plan(now)?;
        }
        if self
            .current_guid
            .as_deref()
            .is_some_and(|current| same_guid(current, &target_guid))
        {
            match temporary_effect {
                TemporaryEffect::Acquire(owner) => {
                    if let Some(plan) = &mut self.temporary_plan {
                        plan.owner = owner;
                    }
                }
                TemporaryEffect::Release => self.temporary_plan = None,
                TemporaryEffect::Preserve => {}
            }
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
        match self.switch_active_verified(&target_guid, temporary_effect) {
            Ok(transition) => {
                self.clear_switch_failure(&target_guid);
                let applied = !same_guid(&transition.previous_guid, &transition.target_guid);
                if applied {
                    self.record_ordinary_switch(&transition, temporary_effect);
                }
                Ok(applied)
            }
            Err(error) => {
                self.record_switch_failure(&target_guid);
                Err(error)
            }
        }
    }

    fn restore_temporary_plan(&mut self, now: Instant) -> Result<bool, String> {
        let Some(plan) = self.temporary_plan.clone() else {
            return Ok(false);
        };
        let decision = self
            .last_decision
            .as_mut()
            .ok_or_else(|| "Temporary restoration has no power-plan decision.".to_owned())?;
        let previous_decision = decision.clone();
        decision.state = plan.owner.state();
        decision.rule_index = None;
        decision.power_plan_guid = Some(plan.original_guid.clone());
        decision.reason = format!(
            "{} no longer selects a plan; restoring the previous power plan.",
            plan.owner.label()
        );
        decision.restore_previous_plan = false;
        if !same_guid(&plan.expected_guid, &plan.original_guid)
            && self.last_switch_attempt.as_ref().is_some_and(|(guid, at)| {
                same_guid(guid, &plan.original_guid)
                    && now.saturating_duration_since(*at) < SWITCH_RETRY_INTERVAL
            })
        {
            return Ok(false);
        }
        self.last_switch_attempt = Some((plan.original_guid.clone(), now));
        match self.restore_active_verified(
            &plan.expected_guid,
            &plan.original_guid,
            TemporaryEffect::Release,
        ) {
            Ok(restored) => {
                self.clear_switch_failure(&plan.original_guid);
                let applied = restored && !same_guid(&plan.expected_guid, &plan.original_guid);
                if !applied {
                    self.last_decision = Some(previous_decision);
                }
                Ok(applied)
            }
            Err(error) => {
                self.record_switch_failure(&plan.original_guid);
                Err(format!(
                    "Cannot restore the previous {} plan: {error}",
                    plan.owner.label()
                ))
            }
        }
    }

    pub(crate) fn reconcile_adaptive(
        &mut self,
        request: AdaptivePowerPlanRequest,
        now: Instant,
    ) -> Result<AdaptivePowerProfile, String> {
        if let Some(due) = self.setup_cleanup_due {
            if now < due {
                return Err("Adaptive plan setup cleanup is pending.".to_owned());
            }
            self.setup_cleanup_due = Some(now + SWITCH_RETRY_INTERVAL);
            self.release_adaptive(now)?;
        }
        self.refresh_active_plan_if_due(now)?;
        self.cleanup_inactive_adaptive_plan()?;

        if self.adaptive_plan.is_none() {
            let original_guid = self.platform.active_guid()?;
            self.observe_active_plan(&original_guid)?;
            let plan_guid = self.platform.duplicate_adaptive_plan(&original_guid)?;
            let values = request.profile.calibrated_power_values(
                request.baseline,
                request.has_efficiency_cores,
                request.background_pressure_profile,
                request.focus_and_launch_profile,
            );
            // Own the duplicate before even metadata or idle-state initialization can fail.
            // Activation failure can be uncertain; cleanup rechecks the actual active plan.
            self.adaptive_plan = Some(ActiveAdaptivePowerPlan {
                original_guid: original_guid.clone(),
                plan_guid: plan_guid.clone(),
                profile: request.profile,
                values: None,
                lower_demand_since: None,
                active: false,
            });
            self.setup_cleanup_due = Some(now + SWITCH_RETRY_INTERVAL);
            let transition = self
                .platform
                .initialize_adaptive_plan(&plan_guid, &original_guid)
                .and_then(|()| self.platform.apply_processor_values(&plan_guid, values))
                .and_then(|()| self.switch_active_verified(&plan_guid, TemporaryEffect::Preserve));
            let transition = match transition {
                Ok(transition) => transition,
                Err(error) => {
                    return Err(adaptive_plan_setup_error(error, self.release_adaptive(now)));
                }
            };
            self.setup_cleanup_due = None;
            self.current_guid = Some(plan_guid.clone());
            self.last_verified_application = Some(plan_guid.clone());
            self.adaptive_plan = Some(ActiveAdaptivePowerPlan {
                original_guid: transition.previous_guid,
                plan_guid,
                profile: request.profile,
                values: Some(values),
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
        let next_values = next_profile.calibrated_power_values(
            request.baseline,
            request.has_efficiency_cores,
            request.background_pressure_profile,
            request.focus_and_launch_profile,
        );
        let values_changed = Some(next_values) != plan.values;
        if values_changed {
            // A failed write may already have changed part of the stored plan.
            plan.values = None;
            self.platform
                .apply_processor_values(&plan.plan_guid, next_values)?;
        }
        if next_profile != plan.profile || values_changed {
            plan.profile = next_profile;
            plan.values = Some(next_values);
            plan.lower_demand_since = None;
        }

        Ok(plan.profile)
    }

    pub(crate) fn release_adaptive(&mut self, now: Instant) -> Result<(), String> {
        self.refresh_active_plan(now)?;
        let Some(mut plan) = self.adaptive_plan.take() else {
            return Ok(());
        };

        if plan.active || self.setup_cleanup_due.is_some() {
            let current_matches = self
                .current_guid
                .as_deref()
                .is_some_and(|current| same_guid(current, &plan.plan_guid));
            if current_matches {
                match self.restore_active_verified(
                    &plan.plan_guid,
                    &plan.original_guid,
                    TemporaryEffect::Preserve,
                ) {
                    Ok(restored) => {
                        if restored {
                            self.current_guid = Some(plan.original_guid.clone());
                            self.last_verified_application = self.ordinary_expected_guid.clone();
                        }
                        plan.active = false;
                    }
                    Err(error) => {
                        self.adaptive_plan = Some(plan);
                        return Err(error);
                    }
                }
            } else {
                // An incomplete clone that was never activated does not break the
                // pre-existing ordinary plan's restoration chain.
                let untouched_source = !plan.active
                    && self.setup_cleanup_due.is_some()
                    && self
                        .current_guid
                        .as_deref()
                        .is_some_and(|current| same_guid(current, &plan.original_guid));
                if !untouched_source {
                    if let Err(error) = self.relinquish_ownership() {
                        self.adaptive_plan = Some(plan);
                        return Err(error);
                    }
                }
                plan.active = false;
            }
        }

        if let Err(error) = self.platform.delete_plan(&plan.plan_guid) {
            self.adaptive_plan = Some(plan);
            return Err(error);
        }
        self.setup_cleanup_due = None;
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
            self.temporary_plan = None;
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

        match self.restore_active_verified(
            &expected_guid,
            &original_guid,
            TemporaryEffect::Preserve,
        ) {
            Ok(restored) => {
                if restored {
                    self.current_guid = Some(original_guid);
                }
                self.last_verified_application = None;
                self.temporary_plan = None;
                self.clear_ordinary_ownership();
                Ok(())
            }
            Err(error) => Err(format!(
                "Cannot restore original power plan {original_guid}: {error}"
            )),
        }
    }

    pub(crate) fn refresh_active_plan_if_due(&mut self, now: Instant) -> Result<(), String> {
        if self.ownership_release_pending
            || self.uncertain_switch.is_some()
            || self
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
        if self.setup_cleanup_due.is_some() {
            return Ok(());
        }
        let guid = plan.plan_guid.clone();
        let actual = self.platform.active_guid()?;
        if same_guid(&actual, &guid) {
            return Err("Inactive Adaptive plan is currently active; cleanup deferred.".to_owned());
        }
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
    target_guid: String,
}

#[derive(Debug, Clone)]
struct TemporaryPowerPlan {
    original_guid: String,
    expected_guid: String,
    owner: TemporaryOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryOwner {
    Focus(Option<u32>),
    RunningApp,
    CpuLoad,
    ActivityIdle,
    ActivityActive,
    Time,
}

impl TemporaryOwner {
    fn from_decision(decision: &DecisionOutcome) -> Result<Self, String> {
        match decision.state {
            DecisionState::ByFocus => Ok(Self::Focus(decision.focus.process_id)),
            DecisionState::ByRunningApp => Ok(Self::RunningApp),
            DecisionState::ByCpuLoad => Ok(Self::CpuLoad),
            DecisionState::ByActivityIdle => Ok(Self::ActivityIdle),
            DecisionState::ByActivityActive => Ok(Self::ActivityActive),
            DecisionState::ByTime => Ok(Self::Time),
            _ => Err("Power-plan restoration was requested without a temporary owner.".into()),
        }
    }

    fn state(self) -> DecisionState {
        match self {
            Self::Focus(_) => DecisionState::ByFocus,
            Self::RunningApp => DecisionState::ByRunningApp,
            Self::CpuLoad => DecisionState::ByCpuLoad,
            Self::ActivityIdle => DecisionState::ByActivityIdle,
            Self::ActivityActive => DecisionState::ByActivityActive,
            Self::Time => DecisionState::ByTime,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Focus(_) => "By Focus",
            Self::RunningApp => "By Running App",
            Self::CpuLoad => "By CPU Load",
            Self::ActivityIdle | Self::ActivityActive => "By Activity",
            Self::Time => "By Time",
        }
    }

    fn observation_unavailable(self, decision: &DecisionOutcome) -> bool {
        let priority = |state| match state {
            DecisionState::ByFocus => 0,
            DecisionState::ByRunningApp => 1,
            DecisionState::ByCpuLoad => 2,
            DecisionState::ByActivityIdle | DecisionState::ByActivityActive => 3,
            DecisionState::ByTime => 4,
            _ => 5,
        };
        let selected_priority = priority(decision.state);
        if (!decision.running_app_known && selected_priority >= 1)
            || (!decision.cpu_load_known && selected_priority >= 2)
            || (!decision.activity_known && selected_priority >= 3)
        {
            return true;
        }
        match self {
            Self::Focus(process_id) => {
                decision.focus.enabled
                    && !decision.focus.known
                    && process_id
                        .zip(decision.focus.process_id)
                        .is_none_or(|(previous, current)| previous == current)
            }
            Self::RunningApp
            | Self::CpuLoad
            | Self::ActivityIdle
            | Self::ActivityActive
            | Self::Time => false,
        }
    }
}

#[derive(Clone, Copy)]
enum TemporaryEffect {
    Preserve,
    Acquire(TemporaryOwner),
    Release,
}

struct UncertainSwitch<D> {
    transition: SwitchTransition,
    recovery: D,
    allow_previous: bool,
    temporary_effect: TemporaryEffect,
}

#[derive(Debug)]
enum CompensationState {
    Previous,
    External(String),
    Uncertain,
}

struct SwitchFailure<D> {
    message: String,
    state: FailedSwitchState<D>,
}

enum FailedSwitchState<D> {
    Previous,
    External(String),
    Pending { recovery: D, allow_previous: bool },
}

fn switch_from_verified<P: PowerPlanPlatform>(
    platform: &mut P,
    previous_guid: String,
    target_guid: &str,
) -> Result<SwitchTransition, SwitchFailure<<P::RecoveryIntent as PowerPlanRecoveryIntent>::Deferred>>
{
    if same_guid(&previous_guid, target_guid) {
        return Ok(SwitchTransition {
            previous_guid,
            target_guid: target_guid.to_owned(),
        });
    }

    let recovery = platform
        .begin_switch(&previous_guid, target_guid)
        .map_err(|message| SwitchFailure {
            message,
            state: FailedSwitchState::Previous,
        })?;
    if let Err(error) = platform.set_active(target_guid) {
        return compensate_failed_switch(
            platform,
            &previous_guid,
            target_guid,
            recovery,
            format!("Power plan application failed: {error}"),
            None,
        );
    }
    match platform.active_guid() {
        Ok(actual_guid) if same_guid(&actual_guid, target_guid) => {
            if let Err((commit_error, recovery)) = recovery.commit() {
                // The helper may have already compacted this transition. Keep its
                // verified target in place: rolling back would make a committed
                // mismatch hide earlier baselines from crash recovery.
                return Err(SwitchFailure {
                    message: format!("Power-plan recovery commit failed: {commit_error}"),
                    state: FailedSwitchState::Pending {
                        recovery,
                        allow_previous: false,
                    },
                });
            }
            Ok(SwitchTransition {
                previous_guid,
                target_guid: target_guid.to_owned(),
            })
        }
        verification => {
            let verification_error = match &verification {
                Ok(actual_guid) => format!(
                    "Power plan verification expected {target_guid}, but Windows reported {actual_guid}."
                ),
                Err(error) => format!("Power plan verification failed: {error}"),
            };
            compensate_failed_switch(
                platform,
                &previous_guid,
                target_guid,
                recovery,
                verification_error,
                verification.ok(),
            )
        }
    }
}

fn compensate_failed_switch<P: PowerPlanPlatform>(
    platform: &mut P,
    previous_guid: &str,
    target_guid: &str,
    recovery: P::RecoveryIntent,
    operation_error: String,
    observed_guid: Option<String>,
) -> Result<SwitchTransition, SwitchFailure<<P::RecoveryIntent as PowerPlanRecoveryIntent>::Deferred>>
{
    let (compensation, state) =
        compensate_switch(platform, previous_guid, target_guid, observed_guid);
    let state = match state {
        CompensationState::Previous => {
            drop(recovery);
            FailedSwitchState::Previous
        }
        CompensationState::External(actual_guid) => {
            drop(recovery);
            FailedSwitchState::External(actual_guid)
        }
        CompensationState::Uncertain => FailedSwitchState::Pending {
            recovery: recovery.defer(),
            allow_previous: true,
        },
    };
    Err(SwitchFailure {
        message: compensated_switch_error(operation_error, compensation),
        state,
    })
}

fn compensate_switch<P: PowerPlanPlatform>(
    platform: &mut P,
    previous_guid: &str,
    target_guid: &str,
    observed_guid: Option<String>,
) -> (Result<(), String>, CompensationState) {
    // An observed external selection terminates this transaction's ownership.
    // Do not overwrite it even if another query would subsequently report our target.
    if let Some(actual_guid) = observed_guid {
        if !same_guid(&actual_guid, previous_guid) && !same_guid(&actual_guid, target_guid) {
            return (Ok(()), CompensationState::External(actual_guid));
        }
    }
    let actual_guid = match platform.active_guid() {
        Ok(actual_guid) => actual_guid,
        Err(error) => return (Err(error), CompensationState::Uncertain),
    };
    if same_guid(&actual_guid, previous_guid) {
        return (Ok(()), CompensationState::Previous);
    }
    if !same_guid(&actual_guid, target_guid) {
        return (Ok(()), CompensationState::External(actual_guid));
    }
    if let Err(error) = platform.set_active(previous_guid) {
        return (Err(error), CompensationState::Uncertain);
    }
    match platform.active_guid() {
        Ok(actual_guid) if same_guid(&actual_guid, previous_guid) => {
            (Ok(()), CompensationState::Previous)
        }
        Ok(actual_guid) if !same_guid(&actual_guid, target_guid) => {
            (Ok(()), CompensationState::External(actual_guid))
        }
        Ok(actual_guid) => (
            Err(format!("Power plan compensation expected {previous_guid}, but Windows reported {actual_guid}.")),
            CompensationState::Uncertain,
        ),
        Err(error) => (Err(error), CompensationState::Uncertain),
    }
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
    use std::{
        cell::RefCell,
        collections::{BTreeSet, VecDeque},
        rc::Rc,
    };

    use super::*;
    use crate::{
        config::{
            ByCpuLoadRule, ByCpuLoadSettings, ByCpuLoadStability, CpuLoadPercent,
            CpuUsageComparison,
        },
        features::power_plan_control::{ByCpuLoadDecision, ByCpuLoadScheduler},
        rules::DecisionState,
    };

    fn switch_active_verified<P: PowerPlanPlatform>(
        platform: &mut P,
        target_guid: &str,
    ) -> Result<SwitchTransition, String> {
        let previous_guid = platform.active_guid()?;
        switch_from_verified(platform, previous_guid, target_guid)
            .map_err(|failure| failure.message)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FailurePoint {
        Begin,
        Apply,
        Verify,
        Commit,
        Forget,
        Delete,
    }

    struct FakeRecoveryIntent {
        events: Rc<RefCell<Vec<String>>>,
        journal: Rc<RefCell<Vec<(String, String)>>>,
        pending: Rc<RefCell<Vec<(String, String)>>>,
        resolution_failure: Rc<RefCell<bool>>,
        transition: (String, String),
        fail_commit: bool,
        completed: bool,
    }

    struct FakeDeferredIntent {
        events: Rc<RefCell<Vec<String>>>,
        journal: Rc<RefCell<Vec<(String, String)>>>,
        pending: Rc<RefCell<Vec<(String, String)>>>,
        resolution_failure: Rc<RefCell<bool>>,
        transition: (String, String),
    }

    impl PowerPlanDeferredRecoveryIntent for FakeDeferredIntent {
        fn resolve(&mut self, applied: bool) -> Result<(), String> {
            self.events.borrow_mut().push(format!("resolve:{applied}"));
            if *self.resolution_failure.borrow() {
                return Err("injected pending resolution failure".to_owned());
            }
            self.pending
                .borrow_mut()
                .retain(|transition| transition != &self.transition);
            if applied {
                self.journal.borrow_mut().push(self.transition.clone());
            }
            Ok(())
        }
    }

    impl PowerPlanRecoveryIntent for FakeRecoveryIntent {
        type Deferred = FakeDeferredIntent;

        fn commit(mut self) -> Result<(), (String, Self::Deferred)> {
            self.events.borrow_mut().push("commit".to_owned());
            if self.fail_commit {
                Err(("injected commit failure".to_owned(), self.defer()))
            } else {
                self.completed = true;
                self.pending
                    .borrow_mut()
                    .retain(|transition| transition != &self.transition);
                self.journal.borrow_mut().push(self.transition.clone());
                Ok(())
            }
        }

        fn defer(mut self) -> Self::Deferred {
            self.completed = true;
            self.events.borrow_mut().push("defer".to_owned());
            FakeDeferredIntent {
                events: Rc::clone(&self.events),
                journal: Rc::clone(&self.journal),
                pending: Rc::clone(&self.pending),
                resolution_failure: Rc::clone(&self.resolution_failure),
                transition: self.transition.clone(),
            }
        }
    }

    impl Drop for FakeRecoveryIntent {
        fn drop(&mut self) {
            if !self.completed {
                self.events.borrow_mut().push("cancel".to_owned());
                self.pending
                    .borrow_mut()
                    .retain(|transition| transition != &self.transition);
            }
        }
    }

    struct FakePlatform {
        active: String,
        query_changes: VecDeque<Option<String>>,
        query_count: usize,
        query_failures: BTreeSet<usize>,
        apply_results: VecDeque<(bool, Result<(), String>)>,
        plans: BTreeSet<String>,
        events: Rc<RefCell<Vec<String>>>,
        journal: Rc<RefCell<Vec<(String, String)>>>,
        pending: Rc<RefCell<Vec<(String, String)>>>,
        resolution_failure: Rc<RefCell<bool>>,
        failure: Option<FailurePoint>,
        verify_wrong_guid: bool,
        next_adaptive: u32,
        processor_values: Option<ProcessorPowerSourceValues>,
        fail_partial_processor_write: bool,
        fail_duplicate: bool,
        initialization_failure: Option<&'static str>,
    }

    impl FakePlatform {
        fn new(active: &str) -> Self {
            Self {
                active: active.to_owned(),
                query_changes: VecDeque::new(),
                query_count: 0,
                query_failures: BTreeSet::new(),
                apply_results: VecDeque::new(),
                plans: BTreeSet::from([active.to_owned()]),
                events: Rc::new(RefCell::new(Vec::new())),
                journal: Rc::new(RefCell::new(Vec::new())),
                pending: Rc::new(RefCell::new(Vec::new())),
                resolution_failure: Rc::new(RefCell::new(false)),
                failure: None,
                verify_wrong_guid: false,
                next_adaptive: 1,
                processor_values: None,
                fail_partial_processor_write: false,
                fail_duplicate: false,
                initialization_failure: None,
            }
        }
    }

    impl PowerPlanPlatform for FakePlatform {
        type RecoveryIntent = FakeRecoveryIntent;

        fn active_guid(&mut self) -> Result<String, String> {
            self.events.borrow_mut().push("query".to_owned());
            self.query_count += 1;
            if let Some(Some(guid)) = self.query_changes.pop_front() {
                self.active = guid;
            }
            if self.query_failures.contains(&self.query_count) {
                return Err("injected active query failure".to_owned());
            }
            if self.failure == Some(FailurePoint::Verify) && self.verify_wrong_guid {
                self.verify_wrong_guid = false;
                self.active = "external".to_owned();
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
            self.pending
                .borrow_mut()
                .push((original_guid.to_owned(), expected_guid.to_owned()));
            Ok(FakeRecoveryIntent {
                events: Rc::clone(&self.events),
                journal: Rc::clone(&self.journal),
                pending: Rc::clone(&self.pending),
                resolution_failure: Rc::clone(&self.resolution_failure),
                transition: (original_guid.to_owned(), expected_guid.to_owned()),
                fail_commit: self.failure == Some(FailurePoint::Commit),
                completed: false,
            })
        }

        fn set_active(&mut self, guid: &str) -> Result<(), String> {
            self.events.borrow_mut().push(format!("apply:{guid}"));
            if let Some((apply, result)) = self.apply_results.pop_front() {
                if apply {
                    self.active = guid.to_owned();
                }
                return result;
            }
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

        fn forget_switches(&mut self) -> Result<(), String> {
            self.events.borrow_mut().push("forget".to_owned());
            if self.failure == Some(FailurePoint::Forget) {
                return Err("injected forget failure".to_owned());
            }
            self.journal.borrow_mut().clear();
            self.pending.borrow_mut().clear();
            Ok(())
        }

        fn duplicate_adaptive_plan(&mut self, _source_guid: &str) -> Result<String, String> {
            if self.fail_duplicate {
                return Err("injected duplication failure".to_owned());
            }
            let guid = format!("adaptive-{}", self.next_adaptive);
            self.next_adaptive += 1;
            self.plans.insert(guid.clone());
            self.events.borrow_mut().push(format!("duplicate:{guid}"));
            Ok(guid)
        }

        fn initialize_adaptive_plan(
            &mut self,
            guid: &str,
            _source_guid: &str,
        ) -> Result<(), String> {
            assert!(self.plans.contains(guid));
            self.events.borrow_mut().push(format!("initialize:{guid}"));
            if let Some(stage) = self.initialization_failure {
                Err(format!("injected {stage} failure"))
            } else {
                Ok(())
            }
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
            values: ProcessorPowerSourceValues,
        ) -> Result<(), String> {
            self.events.borrow_mut().push(format!("configure:{guid}"));
            if std::mem::take(&mut self.fail_partial_processor_write) {
                self.processor_values.get_or_insert(values).ac = values.ac;
                return Err("injected partial processor write".into());
            }
            self.processor_values = Some(values);
            Ok(())
        }
    }

    #[test]
    fn incomplete_initialization_retains_the_duplicate_until_cleanup_succeeds() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.platform.initialization_failure = Some("initialization");
        controller.platform.failure = Some(FailurePoint::Delete);
        let error = controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap_err();
        assert!(error.contains("initialization"));
        assert!(error.contains("delete"));
        assert_eq!(
            controller.adaptive_plan.as_ref().unwrap().plan_guid,
            "adaptive-1"
        );
        assert!(!controller.adaptive_active());
        assert_eq!(controller.platform.active, "original");
        assert!(controller.platform.processor_values.is_none());
        assert!(!controller
            .platform
            .events
            .borrow()
            .iter()
            .any(|event| { event.starts_with("configure:") || event.starts_with("apply:") }));
        assert!(controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .is_err());
        assert_eq!(controller.platform.next_adaptive, 2);
        let events_before_retry = controller.platform.events.borrow().len();
        controller
            .retry_pending_cleanup(now + Duration::from_secs(1))
            .unwrap();
        assert_eq!(
            controller.platform.events.borrow().len(),
            events_before_retry
        );
        assert!(controller
            .retry_pending_cleanup(now + SWITCH_RETRY_INTERVAL)
            .is_err());
        assert_eq!(controller.platform.next_adaptive, 2);
        controller.platform.failure = None;
        controller
            .retry_pending_cleanup(now + SWITCH_RETRY_INTERVAL * 2)
            .unwrap();
        assert!(controller.adaptive_plan.is_none());
        assert!(controller.cleanup_retry_delay(now).is_none());
        assert!(!controller.platform.plans.contains("adaptive-1"));
        assert_eq!(controller.platform.next_adaptive, 2);
        controller.platform.initialization_failure = None;
        controller
            .reconcile_adaptive(adaptive_request(), now + SWITCH_RETRY_INTERVAL * 3)
            .unwrap();
        assert_eq!(controller.platform.active, "adaptive-2");
        controller
            .shutdown(now + SWITCH_RETRY_INTERVAL * 4)
            .unwrap();
    }

    #[test]
    fn failed_duplication_never_initializes_or_claims_a_plan() {
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.platform.fail_duplicate = true;
        assert!(controller
            .reconcile_adaptive(adaptive_request(), Instant::now())
            .is_err());
        assert!(controller.adaptive_plan.is_none());
        assert!(controller.cleanup_retry_delay(Instant::now()).is_none());
        assert_eq!(
            controller.platform.plans,
            BTreeSet::from(["original".to_owned()])
        );
        assert!(!controller
            .platform
            .events
            .borrow()
            .iter()
            .any(|event| event.starts_with("initialize:") || event.starts_with("delete:")));
    }

    #[test]
    fn incomplete_initialization_preserves_ordinary_restoration_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller.platform.initialization_failure = Some("name");
        controller.platform.failure = Some(FailurePoint::Delete);
        assert!(controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .is_err());
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("original")
        );
        assert_eq!(
            controller.ordinary_expected_guid.as_deref(),
            Some("ordinary")
        );
        assert_eq!(controller.platform.active, "ordinary");
        controller.platform.failure = None;
        controller.shutdown(now + Duration::from_secs(2)).unwrap();
        assert_eq!(controller.platform.active, "original");
        assert!(!controller.platform.plans.contains("adaptive-1"));
    }

    #[test]
    fn external_change_during_incomplete_initialization_is_not_overwritten() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller.platform.initialization_failure = Some("description");
        controller.platform.failure = Some(FailurePoint::Delete);
        assert!(controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .is_err());
        controller.platform.active = "external".to_owned();
        controller.platform.failure = None;
        controller
            .retry_pending_cleanup(now + SWITCH_RETRY_INTERVAL * 2)
            .unwrap();
        controller
            .shutdown(now + SWITCH_RETRY_INTERVAL * 3)
            .unwrap();
        assert_eq!(controller.platform.active, "external");
        assert!(!controller.platform.plans.contains("adaptive-1"));
    }

    #[test]
    fn successful_initialization_precedes_processor_configuration_and_activation() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        let events = controller.platform.events.borrow();
        let duplicate = events
            .iter()
            .position(|event| event == "duplicate:adaptive-1")
            .unwrap();
        let initialize = events
            .iter()
            .position(|event| event == "initialize:adaptive-1")
            .unwrap();
        let configure = events
            .iter()
            .position(|event| event == "configure:adaptive-1")
            .unwrap();
        let activate = events
            .iter()
            .position(|event| event == "apply:adaptive-1")
            .unwrap();
        assert!(duplicate < initialize && initialize < configure && configure < activate);
        drop(events);
        controller.shutdown(now + Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn failed_setup_keeps_the_guid_until_cleanup_succeeds() {
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.platform.failure = Some(FailurePoint::Delete);
        controller.platform.fail_partial_processor_write = true;
        let now = Instant::now();
        assert!(controller
            .reconcile_adaptive(adaptive_request(), now)
            .is_err());
        assert_eq!(
            controller.adaptive_plan.as_ref().unwrap().plan_guid,
            "adaptive-1"
        );
        assert!(controller
            .reconcile_adaptive(adaptive_request(), now + Duration::from_secs(1))
            .is_err());
        assert_eq!(controller.platform.next_adaptive, 2);
        assert!(controller
            .retry_pending_cleanup(now + Duration::from_secs(16))
            .is_err());
        assert_eq!(controller.platform.next_adaptive, 2);
        controller.platform.failure = None;
        controller
            .retry_pending_cleanup(now + Duration::from_secs(32))
            .unwrap();
        assert!(controller.adaptive_plan.is_none());
        assert!(!controller.platform.plans.contains("adaptive-1"));
        assert_eq!(controller.platform.active, "original");
    }

    fn decision(target: Option<&str>) -> DecisionOutcome {
        DecisionOutcome {
            rule_index: None,
            power_plan_guid: target.map(str::to_owned),
            state: DecisionState::NoPowerPlanSelected,
            reason: "test decision".to_owned(),
            focus: Default::default(),
            restore_previous_plan: false,
            running_app_known: true,
            cpu_load_known: true,
            activity_known: true,
        }
    }

    fn restoration_observation() -> DecisionOutcome {
        let mut decision = decision(None);
        decision.restore_previous_plan = true;
        decision
    }

    fn focus_decision(target: &str, process_id: u32) -> DecisionOutcome {
        let mut decision = decision(Some(target));
        decision.state = DecisionState::ByFocus;
        decision.focus.enabled = true;
        decision.focus.known = true;
        decision.focus.process_id = Some(process_id);
        decision.restore_previous_plan = true;
        decision
    }

    fn focus_observation(known: bool, process_id: Option<u32>) -> DecisionOutcome {
        let mut decision = restoration_observation();
        decision.focus.enabled = true;
        decision.focus.known = known;
        decision.focus.process_id = process_id;
        decision.restore_previous_plan = true;
        decision
    }

    fn temporary_decision(target: &str, state: DecisionState) -> DecisionOutcome {
        let mut decision = decision(Some(target));
        decision.state = state;
        decision.restore_previous_plan = true;
        decision
    }

    #[test]
    fn running_app_handoffs_restore_the_plan_before_the_first_app_took_over() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(
                temporary_decision("first", DecisionState::ByRunningApp),
                now,
            )
            .unwrap();
        controller
            .reconcile_ordinary(
                temporary_decision("second", DecisionState::ByRunningApp),
                now,
            )
            .unwrap();
        assert!(controller.has_temporary_plan());
        assert!(!controller.has_focus_plan());
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert!(!controller.has_temporary_plan());
        assert_eq!(
            controller.status().decision_state,
            Some(DecisionState::ByRunningApp)
        );
    }

    #[test]
    fn unavailable_app_observations_defer_release_even_after_focus_preemption() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(
                temporary_decision("running", DecisionState::ByRunningApp),
                now,
            )
            .unwrap();
        let mut unknown = restoration_observation();
        unknown.running_app_known = false;
        assert!(!controller.reconcile_ordinary(unknown.clone(), now).unwrap());
        assert_eq!(controller.platform.active, "running");
        assert!(controller.temporary_release_retry_delay().is_some());
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        assert!(!controller.reconcile_ordinary(unknown, now).unwrap());
        assert_eq!(controller.platform.active, "focused");
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn time_windows_share_a_baseline_across_focus_and_running_app_handoffs() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        for (target, state) in [
            ("scheduled", DecisionState::ByTime),
            ("running", DecisionState::ByRunningApp),
        ] {
            controller
                .reconcile_ordinary(temporary_decision(target, state), now)
                .unwrap();
        }
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        assert!(controller
            .reconcile_ordinary(
                temporary_decision("next window", DecisionState::ByTime),
                now
            )
            .unwrap());
        assert_eq!(controller.platform.active, "next window");
        assert_eq!(
            controller.temporary_plan.as_ref().unwrap().original_guid,
            "original"
        );
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert_eq!(
            controller.status().decision_state,
            Some(DecisionState::ByTime)
        );
    }

    #[test]
    fn cpu_and_activity_restore_only_after_known_release() {
        for state in [
            DecisionState::ByCpuLoad,
            DecisionState::ByActivityIdle,
            DecisionState::ByActivityActive,
        ] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(temporary_decision("temporary", state), now)
                .unwrap();
            let mut unknown = restoration_observation();
            unknown.cpu_load_known = state != DecisionState::ByCpuLoad;
            unknown.activity_known = !matches!(
                state,
                DecisionState::ByActivityIdle | DecisionState::ByActivityActive
            );
            assert!(!controller.reconcile_ordinary(unknown, now).unwrap());
            assert_eq!(controller.platform.active, "temporary");
            assert!(controller.temporary_release_retry_delay().is_some());
            assert!(controller
                .reconcile_ordinary(restoration_observation(), now)
                .unwrap());
            assert_eq!(controller.platform.active, "original");
            assert_eq!(controller.status().decision_state, Some(state));
        }
    }

    #[test]
    fn ended_cpu_condition_under_focus_does_not_restore_the_stale_cpu_target() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(temporary_decision("cpu", DecisionState::ByCpuLoad), now)
            .unwrap();
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        let mut unknown = restoration_observation();
        unknown.cpu_load_known = false;
        assert!(!controller.reconcile_ordinary(unknown, now).unwrap());
        assert_eq!(controller.platform.active, "focused");
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn disabling_restoration_keeps_the_selected_plan_and_ordinary_safety_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(temporary_decision("scheduled", DecisionState::ByTime), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Commit);
        assert!(controller
            .reconcile_ordinary(decision(Some("persistent")), now)
            .is_err());
        assert!(controller.has_temporary_plan());
        assert!(controller.temporary_plan.is_none());
        controller.platform.failure = None;
        assert!(!controller
            .reconcile_ordinary(decision(Some("persistent")), now)
            .unwrap());
        assert!(!controller.has_temporary_plan());
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "persistent");
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn deferred_acquisition_keeps_the_episode_baseline_for_each_temporary_owner() {
        for state in [
            DecisionState::ByRunningApp,
            DecisionState::ByTime,
            DecisionState::ByCpuLoad,
            DecisionState::ByActivityIdle,
            DecisionState::ByActivityActive,
        ] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller.platform.failure = Some(FailurePoint::Commit);
            assert!(controller
                .reconcile_ordinary(temporary_decision("temporary", state), now)
                .is_err());
            assert!(controller.has_temporary_plan());
            assert!(controller.temporary_plan.is_none());
            controller.platform.failure = None;
            assert!(controller
                .reconcile_ordinary(restoration_observation(), now)
                .unwrap());
            assert_eq!(controller.platform.active, "original");
            assert!(!controller.has_temporary_plan());
            assert_eq!(controller.status().decision_state, Some(state));
        }
    }

    #[test]
    fn focus_handoffs_restore_the_immediate_baseline_then_shutdown_restores_the_original() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller
            .reconcile_ordinary(focus_decision("first", 10), now)
            .unwrap();
        controller
            .reconcile_ordinary(focus_decision("second", 20), now)
            .unwrap();
        // Two focused apps may use the same plan without replacing the baseline.
        controller
            .reconcile_ordinary(focus_decision("second", 30), now)
            .unwrap();
        assert_eq!(
            controller.temporary_plan.as_ref().unwrap().owner,
            TemporaryOwner::Focus(Some(30))
        );

        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "ordinary");
        assert!(!controller.has_focus_plan());
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("original")
        );
        assert_eq!(
            controller.status().decision_state,
            Some(DecisionState::ByFocus)
        );
        assert_eq!(controller.status().target_guid.as_deref(), Some("ordinary"));
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn focus_handoff_back_to_the_baseline_does_not_report_a_restore_on_blur() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        controller
            .reconcile_ordinary(focus_decision("original", 20), now)
            .unwrap();
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert!(!controller.has_focus_plan());
        assert!(controller.status().target_guid.is_none());
        assert!(controller.status().owner.is_none());
    }

    #[test]
    fn disabling_restoration_after_no_match_or_unknown_data_does_not_restore() {
        for unknown in [false, true] {
            for pending_commit in [false, true] {
                let now = Instant::now();
                let mut controller =
                    PowerPlanController::with_platform(FakePlatform::new("original"));
                if pending_commit {
                    controller.platform.failure = Some(FailurePoint::Commit);
                }
                assert_eq!(
                    controller
                        .reconcile_ordinary(focus_decision("focused", 10), now)
                        .is_err(),
                    pending_commit
                );
                let mut disabled = decision(None);
                if unknown {
                    disabled.focus.enabled = true;
                    disabled.focus.process_id = Some(10);
                    disabled.running_app_known = false;
                    disabled.cpu_load_known = false;
                    disabled.activity_known = false;
                }
                if pending_commit {
                    *controller.platform.resolution_failure.borrow_mut() = true;
                    assert!(controller
                        .reconcile_ordinary(disabled.clone(), now)
                        .is_err());
                    assert!(controller.has_temporary_plan());
                    controller.platform.failure = None;
                    *controller.platform.resolution_failure.borrow_mut() = false;
                }
                assert!(!controller.reconcile_ordinary(disabled, now).unwrap());
                assert_eq!(controller.platform.active, "focused");
                assert!(!controller.has_temporary_plan());
                assert!(controller.platform.pending.borrow().is_empty());
                assert_eq!(
                    controller.ordinary_original_guid.as_deref(),
                    Some("original")
                );
                // Re-enabling restoration cannot revive an abandoned episode.
                assert!(!controller
                    .reconcile_ordinary(restoration_observation(), now)
                    .unwrap());
                assert_eq!(controller.platform.active, "focused");
                controller.shutdown(now).unwrap();
                assert_eq!(controller.platform.active, "original");
            }
        }
    }

    #[test]
    fn canceling_a_pending_return_survives_reenable_and_both_recovery_outcomes() {
        for at_previous in [false, true] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(focus_decision("first", 10), now)
                .unwrap();
            if at_previous {
                controller.platform.apply_results = VecDeque::from([
                    (true, Err("application failed after mutation".into())),
                    (true, Err("compensation failed after mutation".into())),
                ]);
            } else {
                controller.platform.failure = Some(FailurePoint::Commit);
            }
            assert!(controller
                .reconcile_ordinary(focus_decision("second", 20), now)
                .is_err());
            *controller.platform.resolution_failure.borrow_mut() = true;
            controller.cancel_temporary_restoration();
            assert!(controller.temporary_plan.is_none());
            assert!(controller.has_temporary_plan());
            assert!(controller.temporary_release_retry_delay().is_some());
            assert!(controller
                .reconcile_ordinary(restoration_observation(), now)
                .is_err());
            assert_eq!(controller.platform.pending.borrow().len(), 1);
            controller.platform.failure = None;
            *controller.platform.resolution_failure.borrow_mut() = false;
            assert!(!controller
                .reconcile_ordinary(restoration_observation(), now)
                .unwrap());
            assert_eq!(
                controller.platform.active,
                if at_previous { "first" } else { "second" }
            );
            assert!(!controller.has_temporary_plan());
            assert!(controller.platform.pending.borrow().is_empty());
            controller.shutdown(now).unwrap();
            assert_eq!(controller.platform.active, "original");
        }
    }

    #[test]
    fn an_already_selected_focus_plan_does_not_claim_a_restoration_baseline() {
        for changes_before_switch in [false, true] {
            let now = Instant::now();
            let initial = if changes_before_switch {
                "original"
            } else {
                "focused"
            };
            let mut controller = PowerPlanController::with_platform(FakePlatform::new(initial));
            if changes_before_switch {
                controller
                    .platform
                    .query_changes
                    .extend([None, Some("focused".into())]);
            }
            assert!(!controller
                .reconcile_ordinary(focus_decision("focused", 10), now)
                .unwrap());
            assert!(!controller.has_focus_plan());
            assert!(controller.ordinary_original_guid.is_none());
            controller
                .reconcile_ordinary(restoration_observation(), now)
                .unwrap();
            assert_eq!(controller.platform.active, "focused");
            assert!(!controller
                .platform
                .events
                .borrow()
                .iter()
                .any(|event| event.starts_with("apply:")));
        }
    }

    #[test]
    fn unreadable_focus_defers_a_lower_priority_target_and_schedules_another_observation() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        assert!(controller.temporary_release_retry_delay().is_none());
        let mut unknown = focus_observation(false, Some(10));
        unknown.power_plan_guid = Some("scheduled".into());
        unknown.state = DecisionState::ByTime;
        assert!(!controller.reconcile_ordinary(unknown, now).unwrap());
        assert_eq!(controller.platform.active, "focused");
        assert_eq!(
            controller.temporary_release_retry_delay(),
            Some(Duration::from_secs(1))
        );
        controller
            .reconcile_ordinary(decision(Some("scheduled")), now)
            .unwrap();
        assert_eq!(controller.platform.active, "scheduled");
        assert!(!controller.has_focus_plan());
    }

    #[test]
    fn focus_entry_rechecks_a_cached_target_and_captures_the_actual_previous_plan() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("focused")), now)
            .unwrap();
        controller.platform.active = "external".into();
        // Ordinary cache still says focused and its refresh deadline has not elapsed.
        assert!(controller
            .reconcile_ordinary(focus_decision("focused", 10), now + Duration::from_secs(1))
            .unwrap());
        assert_eq!(
            controller.temporary_plan.as_ref().unwrap().original_guid,
            "external"
        );
        assert_eq!(controller.platform.active, "focused");
        controller
            .reconcile_ordinary(restoration_observation(), now + Duration::from_secs(2))
            .unwrap();
        assert_eq!(controller.platform.active, "external");
        assert!(!controller.has_focus_plan());
        assert!(controller.ordinary_original_guid.is_none());
    }

    #[test]
    fn selected_else_and_active_plans_preempt_restoration_until_they_yield() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        for (target, state) in [
            ("cpu-else", DecisionState::ByCpuLoad),
            ("active", DecisionState::ByActivityActive),
        ] {
            assert!(controller
                .reconcile_ordinary(temporary_decision(target, state), now)
                .unwrap());
            assert_eq!(controller.platform.active, target);
            assert_eq!(
                controller.temporary_plan.as_ref().unwrap().original_guid,
                "original"
            );
        }
        assert!(!controller
            .platform
            .events
            .borrow()
            .iter()
            .any(|event| event == "apply:original"));
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert_eq!(
            controller.status().decision_state,
            Some(DecisionState::ByActivityActive)
        );
    }

    #[test]
    fn uncertain_focus_samples_defer_but_known_nonmatches_and_stop_conditions_release() {
        for (known, process_id, enabled, state, restore) in [
            (false, None, true, DecisionState::NoPowerPlanSelected, false),
            (
                false,
                Some(10),
                true,
                DecisionState::NoPowerPlanSelected,
                false,
            ),
            (
                false,
                Some(20),
                true,
                DecisionState::NoPowerPlanSelected,
                true,
            ),
            (
                true,
                Some(10),
                true,
                DecisionState::NoPowerPlanSelected,
                true,
            ),
            (
                false,
                Some(10),
                false,
                DecisionState::NoPowerPlanSelected,
                true,
            ),
            (false, None, true, DecisionState::Disabled, true),
            (false, None, true, DecisionState::PausedWhilePluggedIn, true),
        ] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(focus_decision("focused", 10), now)
                .unwrap();
            let mut observation = focus_observation(known, process_id);
            observation.state = state;
            observation.focus.enabled = enabled;
            assert_eq!(
                controller.reconcile_ordinary(observation, now).unwrap(),
                restore
            );
            assert_eq!(
                controller.platform.active,
                if restore { "original" } else { "focused" }
            );
            assert_eq!(controller.has_focus_plan(), !restore);
        }
    }

    #[test]
    fn external_plan_changes_before_or_during_focus_release_are_preserved() {
        for change_during_final_query in [false, true] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(focus_decision("focused", 10), now)
                .unwrap();
            if change_during_final_query {
                controller
                    .platform
                    .query_changes
                    .push_back(Some("external".into()));
            } else {
                controller.platform.active = "external".into();
                controller.refresh_active_plan(now).unwrap();
            }
            assert!(!controller
                .reconcile_ordinary(restoration_observation(), now)
                .unwrap());
            assert_eq!(controller.platform.active, "external");
            assert!(!controller.has_focus_plan());
            assert!(controller.ordinary_original_guid.is_none());
            assert!(controller.status().target_guid.is_none());
            assert!(controller.status().owner.is_none());
        }
    }

    #[test]
    fn failed_focus_restore_retains_ownership_and_retries_after_the_interval() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Apply);
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .is_err());
        assert_eq!(controller.platform.active, "focused");
        assert!(controller.has_focus_plan());
        assert!(controller.temporary_release_retry_delay().is_some());
        let attempts = controller
            .platform
            .events
            .borrow()
            .iter()
            .filter(|event| *event == "apply:original")
            .count();
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now + SWITCH_RETRY_INTERVAL / 2)
            .unwrap());
        assert_eq!(
            controller
                .platform
                .events
                .borrow()
                .iter()
                .filter(|event| *event == "apply:original")
                .count(),
            attempts
        );
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now + SWITCH_RETRY_INTERVAL)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert!(!controller.has_focus_plan());
    }

    #[test]
    fn uncertain_focus_activation_commits_the_baseline_before_releasing_it() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.platform.failure = Some(FailurePoint::Commit);
        assert!(controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .is_err());
        assert!(controller.has_focus_plan());
        assert!(controller.temporary_plan.is_none());
        assert!(controller.temporary_release_retry_delay().is_some());
        controller.platform.failure = None;
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
        assert!(!controller.has_focus_plan());
        assert!(controller.uncertain_switch.is_none());
        assert!(controller.platform.pending.borrow().is_empty());
    }

    #[test]
    fn uncertain_focus_release_preserves_the_older_ordinary_restoration_chain() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Commit);
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .is_err());
        assert_eq!(controller.platform.active, "ordinary");
        assert!(controller.has_focus_plan());
        controller.platform.failure = None;
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert!(!controller.has_focus_plan());
        assert_eq!(
            controller.ordinary_expected_guid.as_deref(),
            Some("ordinary")
        );
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn adaptive_ownership_defers_focus_restoration_until_released() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "adaptive-1");
        assert!(controller.has_focus_plan());
        controller.release_adaptive(now).unwrap();
        assert_eq!(controller.platform.active, "focused");
        assert!(controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn disabling_restoration_during_adaptive_ownership_abandons_the_old_episode() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(focus_decision("focused", 10), now)
            .unwrap();
        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        assert!(!controller.reconcile_ordinary(decision(None), now).unwrap());
        assert!(!controller.has_temporary_plan());
        assert_eq!(controller.platform.active, "adaptive-1");
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        controller.release_adaptive(now).unwrap();
        assert_eq!(controller.platform.active, "focused");
        assert!(!controller
            .reconcile_ordinary(restoration_observation(), now)
            .unwrap());
        assert_eq!(controller.platform.active, "focused");
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    fn cpu_load_settings() -> ByCpuLoadSettings {
        ByCpuLoadSettings {
            enabled: true,
            rules: vec![ByCpuLoadRule {
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: CpuLoadPercent::new(75).unwrap(),
                duration_seconds: 0,
                stability: Some(ByCpuLoadStability {
                    exit_threshold_percent: CpuLoadPercent::new(65).unwrap(),
                    exit_upper_threshold_percent: None,
                    minimum_hold_seconds: 30,
                }),
                power_plan_guid: Some("managed".into()),
                ..ByCpuLoadRule::default()
            }],
        }
    }

    fn cpu_load_decision(candidate: &ByCpuLoadDecision) -> DecisionOutcome {
        DecisionOutcome {
            rule_index: Some(candidate.rule_index),
            power_plan_guid: Some(candidate.power_plan_guid.clone()),
            state: DecisionState::ByCpuLoad,
            reason: "test CPU load decision".into(),
            focus: Default::default(),
            restore_previous_plan: false,
            running_app_known: true,
            cpu_load_known: true,
            activity_known: true,
        }
    }

    fn adaptive_request() -> AdaptivePowerPlanRequest {
        AdaptivePowerPlanRequest {
            profile: AdaptivePowerProfile::Responsive,
            baseline: ProcessorPowerValues::for_preset(
                crate::power::ProcessorPowerPreset::Balanced,
            ),
            has_efficiency_cores: false,
            background_pressure_profile: AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
            focus_and_launch_profile: AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
        }
    }

    #[test]
    fn status_reports_failure_until_target_succeeds() {
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.last_decision = Some(decision(Some("plan")));
        assert!(!controller.status().apply_failed);
        controller.record_switch_failure("PLAN");
        assert!(controller.status().apply_failed);
        controller.clear_switch_failure("plan");
        assert!(!controller.status().apply_failed);
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
    fn verification_of_an_external_plan_preserves_it_and_cancels_the_attempt() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Verify);
        let events = Rc::clone(&platform.events);

        assert!(switch_active_verified(&mut platform, "target").is_err());

        assert_eq!(platform.active, "external");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "cancel"
            ]
        );
    }

    #[test]
    fn application_failure_at_the_original_plan_cancels_without_another_setter() {
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
                "query",
                "cancel"
            ]
        );
    }

    #[test]
    fn recovery_commit_failure_keeps_the_verified_target_for_the_helper() {
        let mut platform = FakePlatform::new("original");
        platform.failure = Some(FailurePoint::Commit);
        let events = Rc::clone(&platform.events);

        assert!(switch_active_verified(&mut platform, "target").is_err());

        assert_eq!(platform.active, "target");
        assert_eq!(
            platform.pending.borrow().as_slice(),
            [("original".to_owned(), "target".to_owned())]
        );
        assert_eq!(
            events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "commit",
                "defer"
            ]
        );
    }

    #[test]
    fn compensation_requires_readback_before_canceling_recovery() {
        let mut platform = FakePlatform::new("original");
        platform.query_failures.insert(2);
        platform.apply_results = VecDeque::from([(true, Ok(())), (true, Ok(()))]);
        assert!(switch_active_verified(&mut platform, "target").is_err());
        assert_eq!(platform.active, "original");
        assert_eq!(
            platform.events.borrow().as_slice(),
            [
                "query",
                "begin:original->target",
                "apply:target",
                "query",
                "query",
                "apply:original",
                "query",
                "cancel"
            ]
        );
        assert!(platform.journal.borrow().is_empty());
        assert!(platform.pending.borrow().is_empty());
    }

    #[test]
    fn unapplied_or_unqueryable_compensation_keeps_the_recovery_edge() {
        for failed_readback in [false, true] {
            let mut platform = FakePlatform::new("original");
            platform.query_failures.insert(2);
            if failed_readback {
                platform.query_failures.insert(4);
            }
            platform.apply_results = VecDeque::from([(true, Ok(())), (false, Ok(()))]);
            assert!(switch_active_verified(&mut platform, "target").is_err());
            assert_eq!(platform.active, "target");
            assert_eq!(
                platform.pending.borrow().as_slice(),
                [("original".to_owned(), "target".to_owned())]
            );
            assert!(platform.journal.borrow().is_empty());
            assert!(!platform
                .events
                .borrow()
                .iter()
                .any(|event| event == "cancel"));
        }
    }

    #[test]
    fn failed_commit_preserves_a_later_external_selection_and_forgets_the_chain() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Commit);
        controller.platform.query_changes =
            VecDeque::from([None, None, Some("external".to_owned())]);
        let events_before = controller.platform.events.borrow().len();
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        assert_eq!(controller.platform.active, "second");
        controller.refresh_active_plan(now).unwrap();
        assert_eq!(controller.platform.active, "external");
        assert!(controller.ordinary_original_guid.is_none());
        assert!(controller.platform.journal.borrow().is_empty());
        assert!(controller.platform.pending.borrow().is_empty());
        assert_eq!(
            &controller.platform.events.borrow()[events_before..],
            [
                "query",
                "begin:first->second",
                "apply:second",
                "query",
                "commit",
                "defer",
                "query",
                "forget"
            ]
        );
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "external");
    }

    #[test]
    fn rejected_commit_must_resolve_before_a_later_switch_can_begin() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Commit);
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        assert_eq!(controller.platform.active, "second");
        assert_eq!(controller.platform.journal.borrow().len(), 1);
        assert_eq!(controller.platform.pending.borrow().len(), 1);
        controller.platform.failure = None;
        *controller.platform.resolution_failure.borrow_mut() = true;
        let events_before = controller.platform.events.borrow().len();
        assert!(controller
            .switch_active_verified("third", TemporaryEffect::Preserve)
            .is_err());
        assert_eq!(
            &controller.platform.events.borrow()[events_before..],
            ["query", "resolve:true"]
        );
        assert_eq!(controller.platform.active, "second");
        *controller.platform.resolution_failure.borrow_mut() = false;
        let events_before = controller.platform.events.borrow().len();
        controller
            .reconcile_ordinary(decision(Some("third")), now)
            .unwrap();
        {
            let events = &controller.platform.events.borrow()[events_before..];
            assert!(
                events
                    .iter()
                    .position(|event| event == "resolve:true")
                    .unwrap()
                    < events
                        .iter()
                        .position(|event| event == "begin:second->third")
                        .unwrap()
            );
        }
        assert!(controller.platform.pending.borrow().is_empty());
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn selecting_the_previous_plan_after_a_failed_commit_relinquishes_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller.platform.failure = Some(FailurePoint::Commit);
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        controller.platform.active = "first".to_owned();
        controller.platform.failure = None;
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "first");
        assert!(controller.ordinary_original_guid.is_none());
        assert!(controller.platform.pending.borrow().is_empty());
        assert!(controller.platform.journal.borrow().is_empty());
    }

    #[test]
    fn unresolved_switch_to_the_target_retains_the_first_ordinary_baseline() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        let query_count = controller.platform.query_count;
        controller.platform.query_failures.insert(query_count + 2);
        controller.platform.apply_results = VecDeque::from([(true, Ok(())), (false, Ok(()))]);
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        assert!(controller.uncertain_switch.is_some());
        assert_eq!(controller.platform.active, "second");
        controller.refresh_active_plan(now).unwrap();
        assert!(controller.uncertain_switch.is_none());
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("original")
        );
        assert_eq!(controller.ordinary_expected_guid.as_deref(), Some("second"));
        assert_eq!(controller.platform.journal.borrow().len(), 2);
        assert!(controller.platform.pending.borrow().is_empty());
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn unresolved_switch_back_at_the_previous_plan_preserves_earlier_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        controller.platform.apply_results = VecDeque::from([
            (true, Err("application failed after mutation".to_owned())),
            (true, Err("compensation failed after mutation".to_owned())),
        ]);
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        assert!(controller.uncertain_switch.is_some());
        assert_eq!(controller.platform.active, "first");
        controller.refresh_active_plan(now).unwrap();
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("original")
        );
        assert_eq!(controller.ordinary_expected_guid.as_deref(), Some("first"));
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
    }

    #[test]
    fn unresolved_external_switch_requires_an_acknowledged_forget_before_reuse() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("first")), now)
            .unwrap();
        let query_count = controller.platform.query_count;
        controller
            .platform
            .query_failures
            .extend([query_count + 2, query_count + 3]);
        assert!(controller
            .switch_active_verified("second", TemporaryEffect::Preserve)
            .is_err());
        controller.platform.active = "external".to_owned();
        controller.platform.failure = Some(FailurePoint::Forget);
        assert!(controller.refresh_active_plan(now).is_err());
        assert!(controller.ownership_release_pending);
        assert_eq!(controller.platform.journal.borrow().len(), 1);
        assert_eq!(controller.platform.pending.borrow().len(), 1);
        controller.platform.active = "second".to_owned();
        controller.platform.failure = None;
        controller.shutdown(now).unwrap();
        assert!(!controller.ownership_release_pending);
        assert!(controller.platform.journal.borrow().is_empty());
        assert!(controller.ordinary_original_guid.is_none());
        assert_eq!(controller.platform.active, "second");
    }

    #[test]
    fn unresolved_observation_or_acknowledgement_blocks_new_mutations_and_restoration() {
        for failed_query in [false, true] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(decision(Some("first")), now)
                .unwrap();
            let query_count = controller.platform.query_count;
            controller
                .platform
                .query_failures
                .extend([query_count + 2, query_count + 3]);
            assert!(controller
                .switch_active_verified("second", TemporaryEffect::Preserve)
                .is_err());
            if failed_query {
                let next_query = controller.platform.query_count + 1;
                controller
                    .platform
                    .query_failures
                    .extend(next_query..next_query + 4);
            } else {
                *controller.platform.resolution_failure.borrow_mut() = true;
            }
            let applications_before = controller
                .platform
                .events
                .borrow()
                .iter()
                .filter(|event| event.starts_with("apply:"))
                .count();
            assert!(controller
                .reconcile_ordinary(decision(Some("third")), now)
                .is_err());
            assert!(controller.shutdown(now).is_err());
            assert_eq!(controller.platform.active, "second");
            assert!(controller.uncertain_switch.is_some());
            assert_eq!(controller.platform.journal.borrow().len(), 1);
            assert_eq!(controller.platform.pending.borrow().len(), 1);
            assert_eq!(
                controller
                    .platform
                    .events
                    .borrow()
                    .iter()
                    .filter(|event| event.starts_with("apply:"))
                    .count(),
                applications_before
            );
            controller.platform.query_failures.clear();
            *controller.platform.resolution_failure.borrow_mut() = false;
            controller.shutdown(now).unwrap();
            assert_eq!(controller.platform.active, "original");
        }
    }

    #[test]
    fn uncertain_adaptive_restoration_keeps_the_ordinary_baseline() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        let query_count = controller.platform.query_count;
        // Release refresh, final ownership query, switch verification, compensation query.
        controller
            .platform
            .query_failures
            .extend([query_count + 3, query_count + 4]);
        assert!(controller.release_adaptive(now).is_err());
        assert_eq!(controller.platform.active, "ordinary");
        assert!(controller.uncertain_switch.is_some());
        controller.shutdown(now).unwrap();
        assert_eq!(controller.platform.active, "original");
        assert!(controller.adaptive_plan.is_none());
        assert!(!controller.platform.plans.contains("adaptive-1"));
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
    fn failed_active_plan_query_is_retried_before_using_the_cached_plan() {
        let start = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.refresh_active_plan_if_due(start).unwrap();
        controller.platform.active = "external".into();
        controller
            .platform
            .query_failures
            .insert(controller.platform.query_count + 1);
        let failed_at = start + ACTIVE_PLAN_REFRESH_INTERVAL;

        assert!(controller.refresh_active_plan_if_due(failed_at).is_err());
        controller
            .refresh_active_plan_if_due(failed_at + Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            controller.status().current_guid.as_deref(),
            Some("external")
        );
        assert_eq!(controller.platform.query_count, 3);
    }

    #[test]
    fn already_effective_cpu_plan_begins_hold_after_confirmation() {
        let start = Instant::now();
        let settings = cpu_load_settings();
        let mut scheduler = ByCpuLoadScheduler::default();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("MANAGED"));
        let candidate = scheduler
            .current_decision_at(&settings, Some(80.0), start)
            .unwrap();
        let confirmed_at = start + Duration::from_secs(20);

        assert!(!controller
            .reconcile_ordinary(cpu_load_decision(&candidate), confirmed_at)
            .unwrap());
        scheduler.confirm_applied_rule(
            Some(candidate.rule_index),
            controller.status().current_guid.as_deref(),
            confirmed_at,
        );

        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(49))
            .is_some());
        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(50))
            .is_none());
        assert!(!controller
            .platform
            .events
            .borrow()
            .iter()
            .any(|event| event.starts_with("apply:")));
    }

    #[test]
    fn failed_and_delayed_cpu_plan_switches_do_not_begin_a_hold() {
        let start = Instant::now();
        let settings = cpu_load_settings();
        let mut scheduler = ByCpuLoadScheduler::default();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller.platform.failure = Some(FailurePoint::Begin);
        let candidate = scheduler
            .current_decision_at(&settings, Some(80.0), start)
            .unwrap();

        assert!(controller
            .reconcile_ordinary(cpu_load_decision(&candidate), start)
            .is_err());
        scheduler.confirm_applied_rule(None, None, start);
        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(1))
            .is_none());

        controller.platform.failure = None;
        let retry_at = start + Duration::from_secs(2);
        let candidate = scheduler
            .current_decision_at(&settings, Some(80.0), retry_at)
            .unwrap();
        assert!(!controller
            .reconcile_ordinary(cpu_load_decision(&candidate), retry_at)
            .unwrap());
        scheduler.confirm_applied_rule(
            Some(candidate.rule_index),
            controller.status().current_guid.as_deref(),
            retry_at,
        );
        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(3))
            .is_none());

        let applied_at = start + SWITCH_RETRY_INTERVAL;
        let candidate = scheduler
            .current_decision_at(&settings, Some(80.0), applied_at)
            .unwrap();
        assert!(controller
            .reconcile_ordinary(cpu_load_decision(&candidate), applied_at)
            .unwrap());
        scheduler.confirm_applied_rule(
            Some(candidate.rule_index),
            controller.status().current_guid.as_deref(),
            applied_at,
        );
        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), applied_at + Duration::from_secs(1))
            .is_some());
    }

    #[test]
    fn higher_priority_same_plan_handoff_restarts_cpu_rule_hold() {
        for state in [DecisionState::ByFocus, DecisionState::ByRunningApp] {
            let start = Instant::now();
            let settings = cpu_load_settings();
            let mut scheduler = ByCpuLoadScheduler::default();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            let candidate = scheduler
                .current_decision_at(&settings, Some(80.0), start)
                .unwrap();
            controller
                .reconcile_ordinary(cpu_load_decision(&candidate), start)
                .unwrap();
            scheduler.confirm_applied_rule(
                Some(0),
                controller.status().current_guid.as_deref(),
                start,
            );

            let preempted_at = start + Duration::from_secs(1);
            let mut higher_priority = decision(Some("managed"));
            higher_priority.state = state;
            assert!(!controller
                .reconcile_ordinary(higher_priority, preempted_at)
                .unwrap());
            scheduler.confirm_applied_rule(
                None,
                controller.status().current_guid.as_deref(),
                preempted_at,
            );
            assert!(scheduler
                .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(2))
                .is_none());

            let resumed_at = start + Duration::from_secs(20);
            let candidate = scheduler
                .current_decision_at(&settings, Some(80.0), resumed_at)
                .unwrap();
            assert!(!controller
                .reconcile_ordinary(cpu_load_decision(&candidate), resumed_at)
                .unwrap());
            scheduler.confirm_applied_rule(
                Some(candidate.rule_index),
                controller.status().current_guid.as_deref(),
                resumed_at,
            );
            assert!(scheduler
                .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(49))
                .is_some());
            assert!(scheduler
                .current_decision_at(&settings, Some(20.0), start + Duration::from_secs(50))
                .is_none());
        }
    }

    #[test]
    fn observing_external_plan_releases_cpu_hold_before_proposing_a_plan() {
        let start = Instant::now();
        let settings = cpu_load_settings();
        let mut scheduler = ByCpuLoadScheduler::default();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        let candidate = scheduler
            .current_decision_at(&settings, Some(80.0), start)
            .unwrap();
        controller
            .reconcile_ordinary(cpu_load_decision(&candidate), start)
            .unwrap();
        scheduler.confirm_applied_rule(Some(0), controller.status().current_guid.as_deref(), start);
        controller.platform.active = "external".into();
        let observed_at = start + ACTIVE_PLAN_REFRESH_INTERVAL;

        controller.refresh_active_plan_if_due(observed_at).unwrap();
        scheduler.observe_active_plan(controller.status().current_guid.as_deref());
        assert!(scheduler
            .current_decision_at(&settings, Some(20.0), observed_at)
            .is_none());
        assert!(!controller
            .reconcile_ordinary(decision(None), observed_at)
            .unwrap());
        assert_eq!(controller.platform.active, "external");
        assert!(controller.platform.journal.borrow().is_empty());
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
        assert!(controller.platform.journal.borrow().is_empty());
    }

    #[test]
    fn returning_to_a_relinquished_plan_does_not_resurrect_recovery_ownership() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("managed")), now)
            .unwrap();
        assert!(!controller.platform.journal.borrow().is_empty());
        controller.platform.active = "external".into();
        controller.refresh_active_plan(now).unwrap();
        controller.platform.active = "managed".into();
        controller.shutdown(now).unwrap();

        assert_eq!(controller.platform.active, "managed");
        assert!(controller.platform.journal.borrow().is_empty());
    }

    #[test]
    fn external_change_before_the_final_restoration_query_is_preserved() {
        for adaptive in [false, true] {
            let now = Instant::now();
            let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
            controller
                .reconcile_ordinary(decision(Some("managed")), now)
                .unwrap();
            if adaptive {
                controller
                    .reconcile_adaptive(adaptive_request(), now)
                    .unwrap();
            }
            controller.platform.events.borrow_mut().clear();
            controller.platform.query_changes = VecDeque::from([None, Some("external".into())]);

            if adaptive {
                controller.release_adaptive(now).unwrap();
                assert!(controller.adaptive_plan.is_none());
                assert!(!controller.platform.plans.contains("adaptive-1"));
            } else {
                controller.restore_ordinary(now).unwrap();
            }

            assert_eq!(controller.platform.active, "external");
            assert!(controller.platform.journal.borrow().is_empty());
            assert!(controller.ordinary_original_guid.is_none());
            assert!(!controller
                .platform
                .events
                .borrow()
                .iter()
                .any(|event| event.starts_with("apply:")));
        }
    }

    #[test]
    fn failed_relinquishment_keeps_ownership_and_blocks_replacement_until_acknowledged() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("managed")), now)
            .unwrap();
        controller.platform.active = "external".into();
        controller.platform.failure = Some(FailurePoint::Forget);
        controller.platform.events.borrow_mut().clear();

        assert!(controller
            .reconcile_ordinary(decision(Some("replacement")), now)
            .is_err());
        assert!(controller.ownership_release_pending);
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("original")
        );
        assert_eq!(controller.platform.journal.borrow().len(), 1);
        assert!(!controller
            .platform
            .events
            .borrow()
            .iter()
            .any(|event| event.starts_with("begin:") || event.starts_with("apply:")));

        // A return to the previous value still requires acknowledgement of the observed break.
        controller.platform.active = "managed".into();
        controller.platform.failure = None;
        controller.platform.events.borrow_mut().clear();
        controller
            .reconcile_ordinary(decision(Some("replacement")), now)
            .unwrap();
        assert!(!controller.ownership_release_pending);
        assert_eq!(
            controller.ordinary_original_guid.as_deref(),
            Some("managed")
        );
        assert_eq!(
            controller.platform.journal.borrow().as_slice(),
            [("managed".to_owned(), "replacement".to_owned())]
        );
        let events = controller.platform.events.borrow();
        assert!(
            events.iter().position(|event| event == "forget").unwrap()
                < events
                    .iter()
                    .position(|event| event == "begin:managed->replacement")
                    .unwrap()
        );
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
        assert_eq!(
            controller.platform.journal.borrow().as_slice(),
            [("external".to_owned(), "second".to_owned())]
        );
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
    fn adaptive_activation_captures_an_external_change_after_duplication() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        controller
            .reconcile_ordinary(decision(Some("ordinary")), now)
            .unwrap();
        controller.platform.query_changes = VecDeque::from([None, Some("external".into())]);

        controller
            .reconcile_adaptive(adaptive_request(), now)
            .unwrap();
        assert_eq!(
            controller.platform.journal.borrow().as_slice(),
            [("external".to_owned(), "adaptive-1".to_owned())]
        );
        controller.release_adaptive(now).unwrap();
        controller.shutdown(now).unwrap();
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
    fn adaptive_boost_tuning_reconfigures_the_active_profile() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        let mut request = adaptive_request();
        request.profile = AdaptivePowerProfile::BackgroundPressure;
        controller.reconcile_adaptive(request, now).unwrap();
        controller.platform.events.borrow_mut().clear();

        request.background_pressure_profile.ac_policy = 70;
        controller
            .reconcile_adaptive(request, now + Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            controller.platform.events.borrow().as_slice(),
            ["configure:adaptive-1"]
        );
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
    #[test]
    fn partial_processor_write_invalidates_cache_and_repairs_previous_values() {
        let now = Instant::now();
        let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
        let mut request = adaptive_request();
        request.profile = AdaptivePowerProfile::BackgroundPressure;
        controller.reconcile_adaptive(request, now).unwrap();
        let original = controller.platform.processor_values.unwrap();
        let mut changed = request;
        changed.background_pressure_profile.ac_policy = 70;
        controller.platform.fail_partial_processor_write = true;
        assert!(controller.reconcile_adaptive(changed, now).is_err());
        assert_ne!(controller.platform.processor_values, Some(original));
        controller.platform.events.borrow_mut().clear();
        controller.reconcile_adaptive(request, now).unwrap();
        assert_eq!(controller.platform.processor_values, Some(original));
        assert_eq!(
            controller.platform.events.borrow().as_slice(),
            ["configure:adaptive-1"]
        );
        controller.platform.events.borrow_mut().clear();
        controller.reconcile_adaptive(request, now).unwrap();
        assert!(controller.platform.events.borrow().is_empty());
    }
}
