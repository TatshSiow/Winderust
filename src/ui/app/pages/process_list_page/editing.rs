use super::*;

impl WinderustApp {
    pub(in crate::ui::app) fn finish_process_quick_action(
        &mut self,
        process_name: &str,
        action: &str,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match result {
            Ok(()) => t!(
                "process_list.quick_action_applied",
                action = action,
                name = process_name
            )
            .to_string(),
            Err(error) => t!(
                "process_list.quick_action_failed",
                action = action,
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }
    pub(in crate::ui::app) fn open_process_details(
        &mut self,
        display_name: String,
        executable_path: String,
        cx: &mut Context<Self>,
    ) {
        self.active_power_plan_picker = None;
        self.process_details = Some(ProcessDetailsDraft {
            display_name,
            executable_path,
        });
        cx.notify();
    }

    pub(in crate::ui::app) fn save_process_details(&mut self, cx: &mut Context<Self>) {
        let details = self.process_details.take();
        if details.is_none() {
            return;
        }
        if !self.save_settings() {
            self.process_details = details;
        }
        self.active_power_plan_picker = None;
        cx.notify();
    }

    pub(in crate::ui::app) fn is_process_list_group_collapsed(
        &self,
        executable_path: &str,
    ) -> bool {
        !self
            .expanded_process_list_groups
            .contains(&process_list_executable_path_group_key(Path::new(
                executable_path,
            )))
    }

    pub(in crate::ui::app) fn toggle_process_list_group(
        &mut self,
        executable_path: String,
        cx: &mut Context<Self>,
    ) {
        let key = process_list_executable_path_group_key(Path::new(&executable_path));
        let expanded = if self.expanded_process_list_groups.remove(&key) {
            false
        } else {
            self.expanded_process_list_groups.insert(key.clone());
            true
        };
        begin_expandable_motion(format!("process-list-group-{key}"), expanded);
        cx.notify();
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

    pub(in crate::ui::app) fn set_process_list_adaptive_engine(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.workload_engine.workload_engine_exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }
    pub(in crate::ui::app) fn set_process_list_process_priority(
        &mut self,
        process_name: String,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_process_priority_rule(&mut self.settings.process_priority, &process_name, priority);
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_thread_priority(
        &mut self,
        process_name: String,
        priority: ProcessThreadPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_thread_priority_rule(&mut self.settings.thread_priority, &process_name, priority);
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_dynamic_priority_boost(
        &mut self,
        process_name: String,
        boost: ProcessDynamicPriorityBoostSetting,
        cx: &mut Context<Self>,
    ) {
        set_dynamic_priority_boost_rule(
            &mut self.settings.dynamic_priority_boost,
            &process_name,
            boost,
        );
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_io_priority(
        &mut self,
        process_name: String,
        priority: ProcessIoPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_io_priority_rule(&mut self.settings.io_priority, &process_name, priority);
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_gpu_priority(
        &mut self,
        process_name: String,
        priority: ProcessGpuPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_gpu_priority_rule(&mut self.settings.gpu_priority, &process_name, priority);
        self.finish_process_list_edit(cx);
    }

    pub(in crate::ui::app) fn set_process_list_memory_priority(
        &mut self,
        process_name: String,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_memory_priority_rule(&mut self.settings.memory_priority, &process_name, priority);
        self.finish_process_list_edit(cx);
    }
}
