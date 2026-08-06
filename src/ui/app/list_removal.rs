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
                remove_at(&mut self.settings.by_foreground.rules, index);
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByTimeRule => {
                remove_at(&mut self.settings.by_time.rules, index);
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::ByCpuLoadRule => {
                remove_at(&mut self.settings.by_cpu_load.rules, index);
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::BackgroundEfficiencyExclusion => {
                remove_at(&mut self.settings.background_efficiency.custom_rules, index);
            }
            ListItemRemovalKind::AppSuspensionRule => {
                if let Some(rule) = self.settings.app_suspension.suspendable_apps.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::AppSuspension(rule.executable_path.clone()));
                }
                remove_at(&mut self.settings.app_suspension.suspendable_apps, index);
            }
            ListItemRemovalKind::CpuSetsSoftRule => {
                if let Some(rule) = self.settings.cpu_sets_soft.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CpuSetsSoft(rule.executable_path.clone()));
                }
                remove_at(&mut self.settings.cpu_sets_soft.rules, index);
            }
            ListItemRemovalKind::ProcessorAffinityHardRule => {
                if let Some(rule) = self.settings.processor_affinity_hard.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::ProcessorAffinityHard(
                            rule.executable_path.clone(),
                        ));
                }
                remove_at(&mut self.settings.processor_affinity_hard.rules, index);
            }
            ListItemRemovalKind::CoreLimiterRule => {
                if let Some(rule) = self.settings.core_limiter.rules.get(index) {
                    self.expanded_rule_cards
                        .remove(&RuleCardTarget::CoreLimiter(rule.executable_path.clone()));
                }
                remove_at(&mut self.settings.core_limiter.rules, index);
            }
            ListItemRemovalKind::ByRunningAppRule => {
                remove_at(&mut self.settings.by_running_app.rules, index);
                self.editing_rule_title = None;
                self.expanded_rule_cards.clear();
            }
            ListItemRemovalKind::WorkloadEngineExclusion => {
                remove_at(
                    &mut self.settings.workload_engine.workload_engine_exclusions,
                    index,
                );
            }
            ListItemRemovalKind::ProcessPriorityExclusion => {
                remove_at(&mut self.settings.process_priority.exclusions, index);
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                remove_at(&mut self.settings.thread_priority.exclusions, index);
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => {
                remove_at(&mut self.settings.dynamic_priority_boost.exclusions, index);
            }
            ListItemRemovalKind::IoPriorityExclusion => {
                remove_at(&mut self.settings.io_priority.exclusions, index);
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                remove_at(&mut self.settings.gpu_priority.exclusions, index);
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                remove_at(&mut self.settings.memory_priority.exclusions, index);
            }
            ListItemRemovalKind::TimerResolutionRule => {
                remove_at(&mut self.settings.timer_resolution.rules, index);
            }
            ListItemRemovalKind::MemoryTrimExclusion => {
                remove_at(&mut self.settings.memory_trim.exclusions, index);
            }
        }
    }
}

fn remove_at<T>(items: &mut Vec<T>, index: usize) {
    if index < items.len() {
        items.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::remove_at;

    #[test]
    fn remove_at_removes_only_the_requested_item_and_ignores_stale_indices() {
        let mut items = vec!["first", "second"];

        remove_at(&mut items, 1);
        remove_at(&mut items, 4);

        assert_eq!(items, ["first"]);
    }
}
