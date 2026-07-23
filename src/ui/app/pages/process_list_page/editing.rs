use super::*;

impl WinderustApp {
    pub(in crate::ui::app) fn is_process_list_group_collapsed(&self, process_name: &str) -> bool {
        !self
            .expanded_process_list_groups
            .contains(&process_list_group_key(process_name))
    }

    pub(in crate::ui::app) fn toggle_process_list_group(
        &mut self,
        process_name: String,
        cx: &mut Context<Self>,
    ) {
        let key = process_list_group_key(&process_name);
        let expanded = if self.expanded_process_list_groups.remove(&key) {
            false
        } else {
            self.expanded_process_list_groups.insert(key.clone());
            true
        };
        begin_expandable_motion(format!("process-list-group-{key}"), expanded);
        cx.notify();
    }

    pub(in crate::ui::app) fn set_process_list_column_visible(
        &mut self,
        column: ProcessListColumn,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if visible {
            self.hidden_process_list_columns.remove(&column)
        } else {
            self.hidden_process_list_columns.insert(column)
        };

        let sort_changed =
            !visible && self.process_list_sort.column == ProcessListSortColumn::Column(column);
        if sort_changed {
            self.process_list_sort = ProcessListSort::default();
        }

        if changed || sort_changed {
            cx.notify();
        }
    }

    pub(in crate::ui::app) fn toggle_process_list_sort(
        &mut self,
        column: ProcessListSortColumn,
        cx: &mut Context<Self>,
    ) {
        self.process_list_sort = self.process_list_sort.toggled_for(column);
        cx.notify();
    }

    pub(in crate::ui::app) fn finish_process_list_edit(&mut self, cx: &mut Context<Self>) {
        self.active_power_plan_picker = None;
        cx.notify();
    }

    pub(in crate::ui::app) fn set_process_list_foreground_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_foreground_power_plan_override(
            &mut self.settings.by_foreground,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_running_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_by_running_app_power_plan_override(
            &mut self.settings.by_running_app,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_background_efficiency(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_background_efficiency_custom_rule(
            &mut self.settings.background_efficiency,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_background_cpu_restriction(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.background_cpu_restriction.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_core_limiter(
        &mut self,
        process_name: String,
        max_logical_processors: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        set_core_limiter_override(
            &mut self.settings.core_limiter,
            &process_name,
            max_logical_processors,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_gpu_priority_included(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.gpu_priority.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_memory_trim(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.memory_trim.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_app_suspension(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_app_suspension_override(&mut self.settings.app_suspension, &process_name, included);
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_timer_resolution(
        &mut self,
        process_name: String,
        desired_100ns: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        set_timer_resolution_override(
            &mut self.settings.timer_resolution,
            &process_name,
            desired_100ns,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn apply_process_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| process_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    pub(in crate::ui::app) fn save_process_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_process_priority_rule(&mut self.settings.process_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.process_priority.enabled {
                "process_list.saved_priority_rule"
            } else {
                "process_list.saved_priority_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_priority_setting_label(priority)
            )
            .to_string();
        }
        cx.notify();
    }

    pub(in crate::ui::app) fn apply_memory_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| memory_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_memory_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_memory_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    pub(in crate::ui::app) fn save_memory_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_memory_priority_rule(&mut self.settings.memory_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.memory_priority.enabled {
                "process_list.saved_memory_rule"
            } else {
                "process_list.saved_memory_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_memory_priority_setting_label(priority)
            )
            .to_string();
        }
        cx.notify();
    }
}
