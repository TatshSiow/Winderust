use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn animated_list_item(
        &self,
        target: ListItemRemovalTarget,
        id: impl Into<SharedString>,
        child: AnyElement,
    ) -> AnyElement {
        animated_list_item_child(
            id,
            child,
            self.pending_list_item_removals.contains_key(&target),
        )
    }

    pub(in crate::ui::app) fn request_list_item_removal(
        &mut self,
        target: ListItemRemovalTarget,
        cx: &mut Context<Self>,
    ) {
        if !ui_animations_enabled() {
            self.pending_list_item_removals.remove(&target);
            self.commit_list_item_removal(target);
            self.shift_pending_list_item_removals_after(target);
            cx.notify();
            return;
        }

        if self.pending_list_item_removals.contains_key(&target) {
            cx.notify();
            return;
        }

        self.pending_list_item_removals
            .insert(target, Instant::now());

        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs_f64(MOTION_FAST_SECONDS)).await;
            let _ = this.update(cx, |app, cx| {
                app.finish_due_list_item_removals();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(in crate::ui::app) fn finish_due_list_item_removals(&mut self) {
        let now = Instant::now();
        let mut due = self
            .pending_list_item_removals
            .iter()
            .filter_map(|(target, started)| {
                (now.duration_since(*started) >= Duration::from_secs_f64(MOTION_FAST_SECONDS))
                    .then_some(*target)
            })
            .collect::<Vec<_>>();

        due.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| b.index().cmp(&a.index())));

        for target in due {
            if self.pending_list_item_removals.remove(&target).is_some() {
                self.commit_list_item_removal(target);
                self.shift_pending_list_item_removals_after(target);
            }
        }
    }

    pub(in crate::ui::app) fn shift_pending_list_item_removals_after(
        &mut self,
        removed: ListItemRemovalTarget,
    ) {
        let mut shifted = HashMap::new();
        for (target, started) in self.pending_list_item_removals.drain() {
            let target = if target.same_list(removed) && target.index() > removed.index() {
                target.with_index(target.index() - 1)
            } else {
                target
            };
            shifted.insert(target, started);
        }
        self.pending_list_item_removals = shifted;
    }

    pub(in crate::ui::app) fn commit_list_item_removal(&mut self, target: ListItemRemovalTarget) {
        let index = target.index();

        match target.kind {
            ListItemRemovalKind::ByForegroundRule => {
                if index < self.settings.by_foreground.rules.len() {
                    self.settings.by_foreground.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByTimeRule => {
                if index < self.settings.by_time.rules.len() {
                    self.settings.by_time.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByCpuLoadRule => {
                if index < self.settings.by_cpu_load.rules.len() {
                    self.settings.by_cpu_load.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::BackgroundEfficiencyExclusion => {
                if index < self.settings.background_efficiency.custom_rules.len() {
                    self.settings
                        .background_efficiency
                        .custom_rules
                        .remove(index);
                }
            }
            ListItemRemovalKind::AppSuspensionRule => {
                if let Some(rule) = self.settings.app_suspension.suspendable_apps.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::AppSuspension(rule.process_name.clone()));
                }
                if index < self.settings.app_suspension.suspendable_apps.len() {
                    self.settings.app_suspension.suspendable_apps.remove(index);
                }
            }
            ListItemRemovalKind::BackgroundCpuExclusion => {
                if index < self.settings.background_cpu_restriction.exclusions.len() {
                    self.settings
                        .background_cpu_restriction
                        .exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::CoreLimiterRule => {
                if let Some(rule) = self.settings.core_limiter.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CoreLimiter(rule.process_name.clone()));
                }
                if index < self.settings.core_limiter.rules.len() {
                    self.settings.core_limiter.rules.remove(index);
                }
            }
            ListItemRemovalKind::ByRunningAppRule => {
                if index < self.settings.by_running_app.rules.len() {
                    self.settings.by_running_app.rules.remove(index);
                }
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::WorkloadEngineExclusion => {
                if index
                    < self
                        .settings
                        .workload_engine
                        .workload_engine_exclusions
                        .len()
                {
                    self.settings
                        .workload_engine
                        .workload_engine_exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::ProcessPriorityExclusion => {
                if index < self.settings.process_priority.exclusions.len() {
                    self.settings.process_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                if index < self.settings.thread_priority.exclusions.len() {
                    self.settings.thread_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => {
                if index < self.settings.dynamic_priority_boost.exclusions.len() {
                    self.settings
                        .dynamic_priority_boost
                        .exclusions
                        .remove(index);
                }
            }
            ListItemRemovalKind::IoPriorityExclusion => {
                if index < self.settings.io_priority.exclusions.len() {
                    self.settings.io_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                if index < self.settings.gpu_priority.exclusions.len() {
                    self.settings.gpu_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                if index < self.settings.memory_priority.exclusions.len() {
                    self.settings.memory_priority.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::TimerResolutionRule => {
                if index < self.settings.timer_resolution.rules.len() {
                    self.settings.timer_resolution.rules.remove(index);
                }
            }
            ListItemRemovalKind::MemoryTrimExclusion => {
                if index < self.settings.memory_trim.exclusions.len() {
                    self.settings.memory_trim.exclusions.remove(index);
                }
            }
            ListItemRemovalKind::CoreSteeringRule => {
                if let Some(rule) = self.settings.core_steering.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CoreSteering(rule.process_name.clone()));
                }
                if index < self.settings.core_steering.rules.len() {
                    self.settings.core_steering.rules.remove(index);
                }
            }
        }
    }
}
