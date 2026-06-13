pub mod action_executor;
pub mod app_resource_adapter;
pub mod decision_engine;
pub mod engine;
pub mod execution_failure;
pub mod model;
pub mod power_plan_adapter;
pub mod resolver;
pub mod restore;

#[allow(unused_imports)]
pub use action_executor::{
    ActionExecution, ActionExecutor, AppLifecycleActionBackend, AppPriorityActionBackend,
    AppResourceActionBackend, GenericActionBackend, PowerPlanActionBackend, SystemCpuActionBackend,
};
#[allow(unused_imports)]
pub use app_resource_adapter::{
    active_app_resource_rules_for_settings, app_suspension_rules, background_cpu_restriction_rules,
    cpu_affinity_rules, cpu_limiter_rules, eco_qos_rules, foreground_responsiveness_rules,
    watchdog_rules,
};
pub use decision_engine::{
    DecisionEngine, DecisionInput, DecisionOutcome, DecisionState, PerformanceModeDecision,
};
#[allow(unused_imports)]
pub use engine::{EngineEvaluation, RuleEngine};
#[allow(unused_imports)]
pub use execution_failure::{
    execution_failure_suppression_threshold, normalize_execution_failure_suppression_threshold,
    set_execution_failure_suppression_threshold, ExecutionFailureState, ExecutionSuppression,
    DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD, MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
};
#[allow(unused_imports)]
pub use model::{
    Action, AffinityPolicy, AppMatcher, AppResourcePolicy, AppStatePolicy, AppliedAction,
    ConflictGroup, DetectorEvent, GenericAppConfig, PowerPlanProfile, PreviousValue,
    ProcessIdentity, Rule, RuleId, RuleProcessPriority, RuntimeProcessInfo, RuntimeState, Trigger,
    PRIORITY_ACTIVITY, PRIORITY_BACKGROUND_APP, PRIORITY_CPU_LOAD, PRIORITY_FALLBACK,
    PRIORITY_FOCUSED_APP, PRIORITY_FOREGROUND_RESPONSIVENESS, PRIORITY_MANUAL_OVERRIDE,
    PRIORITY_RUNNING_APP, PRIORITY_SAFETY, PRIORITY_SCHEDULE, PRIORITY_WATCHDOG,
};
#[allow(unused_imports)]
pub use power_plan_adapter::{
    active_power_plan_rules_for_context, decision_outcome_to_rule,
    resolved_power_plan_action_for_context, resolved_power_plan_action_for_decision,
    resolved_power_plan_guid_for_context, resolved_power_plan_guid_for_decision,
    resolved_power_plan_resolved_action_for_context,
};
#[allow(unused_imports)]
pub use resolver::{PriorityResolver, ResolvedAction};
#[allow(unused_imports)]
pub use restore::AppliedActionStore;
