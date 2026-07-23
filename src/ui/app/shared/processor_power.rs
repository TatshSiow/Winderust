use crate::ui::app::*;

impl WinderustApp {
    fn refresh_processor_power_target_plan_personality(&mut self) -> bool {
        let personality = self
            .processor_power_target_plan()
            .and_then(|plan| self.power.read_plan_personality(&plan.guid).ok());
        if self.processor_power_target_plan_personality == personality {
            return false;
        }

        self.processor_power_target_plan_personality = personality;
        true
    }

    pub(in crate::ui::app) fn ensure_processor_power_target_plan(&mut self) {
        let target_still_available = self
            .processor_power_target_plan_guid
            .as_deref()
            .is_some_and(|target| {
                self.plans
                    .iter()
                    .any(|plan| plan.guid.eq_ignore_ascii_case(target))
            });
        if target_still_available {
            return;
        }

        self.processor_power_target_plan_guid = self
            .current_plan
            .as_ref()
            .or_else(|| self.plans.first())
            .map(|plan| plan.guid.clone());
    }

    pub(in crate::ui::app) fn processor_power_target_plan(&self) -> Option<PowerPlan> {
        self.processor_power_target_plan_guid
            .as_deref()
            .and_then(|target| {
                self.plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(target))
            })
            .cloned()
            .or_else(|| self.current_plan.clone())
    }

    fn set_processor_power_target_plan(&mut self, guid: String) {
        self.processor_power_target_plan_guid = Some(guid);
        self.active_power_plan_picker = None;
        self.sync_processor_power_values_from_target_plan(true);
    }

    pub(in crate::ui::app) fn set_processor_power_target_plan_option(
        &mut self,
        guid: Option<String>,
    ) {
        if let Some(guid) = guid {
            self.set_processor_power_target_plan(guid);
        } else {
            self.active_power_plan_picker = None;
        }
    }

    pub(in crate::ui::app) fn sync_processor_power_values_from_target_plan(
        &mut self,
        force: bool,
    ) -> bool {
        self.refresh_processor_power_target_plan_personality();
        let Some(plan) = self.processor_power_target_plan() else {
            self.processor_power_loaded_plan_guid = None;
            return false;
        };
        let same_plan = self
            .processor_power_loaded_plan_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(&plan.guid));
        if !force && same_plan && self.processor_power_dirty {
            return true;
        }

        match self.power.read_processor_power_values(&plan.guid) {
            Ok(values) => {
                self.set_processor_power_values(values.normalized());
                self.processor_power_loaded_plan_guid = Some(plan.guid);
                self.processor_power_dirty = false;
                true
            }
            Err(err) => {
                self.status_message = err;
                false
            }
        }
    }

    pub(in crate::ui::app) fn processor_power_values(&self) -> ProcessorPowerAcDcValues {
        ProcessorPowerAcDcValues::new(
            ProcessorPowerValues::new_with_boost_mode(
                self.processor_power_ac_core_parking_min as u32,
                self.processor_power_ac_performance_min as u32,
                self.processor_power_ac_performance_max as u32,
                self.processor_power_ac_boost_policy as u32,
                self.processor_power_ac_boost_mode,
            ),
            ProcessorPowerValues::new_with_boost_mode(
                self.processor_power_dc_core_parking_min as u32,
                self.processor_power_dc_performance_min as u32,
                self.processor_power_dc_performance_max as u32,
                self.processor_power_dc_boost_policy as u32,
                self.processor_power_dc_boost_mode,
            ),
        )
        .normalized()
    }

    pub(in crate::ui::app) fn set_processor_power_values(
        &mut self,
        values: ProcessorPowerAcDcValues,
    ) {
        let values = values.normalized();
        self.processor_power_ac_core_parking_min = values.ac.core_parking_min as u64;
        self.processor_power_ac_performance_min = values.ac.performance_min as u64;
        self.processor_power_ac_performance_max = values.ac.performance_max as u64;
        self.processor_power_ac_boost_policy = values.ac.boost_policy as u64;
        self.processor_power_ac_boost_mode = values.ac.boost_mode;
        self.processor_power_dc_core_parking_min = values.dc.core_parking_min as u64;
        self.processor_power_dc_performance_min = values.dc.performance_min as u64;
        self.processor_power_dc_performance_max = values.dc.performance_max as u64;
        self.processor_power_dc_boost_policy = values.dc.boost_policy as u64;
        self.processor_power_dc_boost_mode = values.dc.boost_mode;
    }

    pub(in crate::ui::app) fn set_processor_power_boost_mode(
        &mut self,
        source: ProcessorPowerSource,
        boost_mode: ProcessorBoostMode,
    ) {
        self.assign_processor_power_boost_mode(source, boost_mode);
        if self.processor_power_link_ac_dc {
            self.assign_processor_power_boost_mode(source.paired(), boost_mode);
        }
        self.active_power_plan_picker = None;
        self.processor_power_dirty = true;
    }

    fn assign_processor_power_boost_mode(
        &mut self,
        source: ProcessorPowerSource,
        boost_mode: ProcessorBoostMode,
    ) {
        match source {
            ProcessorPowerSource::Ac => self.processor_power_ac_boost_mode = boost_mode,
            ProcessorPowerSource::Dc => self.processor_power_dc_boost_mode = boost_mode,
        }
    }

    pub(in crate::ui::app) fn set_processor_power_slider_value(
        &mut self,
        slider: ProcessorPowerSlider,
        value: u64,
    ) {
        let value = value.min(100);
        self.assign_processor_power_slider_value(slider, value);
        if self.processor_power_link_ac_dc {
            self.assign_processor_power_slider_value(slider.paired_power_source(), value);
        }
        self.processor_power_dirty = true;
    }

    fn assign_processor_power_slider_value(&mut self, slider: ProcessorPowerSlider, value: u64) {
        match slider {
            ProcessorPowerSlider::AcCoreParkingMin => {
                self.processor_power_ac_core_parking_min = value;
            }
            ProcessorPowerSlider::AcPerformanceMin => {
                self.processor_power_ac_performance_min = value;
            }
            ProcessorPowerSlider::AcPerformanceMax => {
                self.processor_power_ac_performance_max = value;
            }
            ProcessorPowerSlider::AcBoostPolicy => {
                self.processor_power_ac_boost_policy = value;
            }
            ProcessorPowerSlider::DcCoreParkingMin => {
                self.processor_power_dc_core_parking_min = value;
            }
            ProcessorPowerSlider::DcPerformanceMin => {
                self.processor_power_dc_performance_min = value;
            }
            ProcessorPowerSlider::DcPerformanceMax => {
                self.processor_power_dc_performance_max = value;
            }
            ProcessorPowerSlider::DcBoostPolicy => {
                self.processor_power_dc_boost_policy = value;
            }
        }
    }

    pub(in crate::ui::app) fn adaptive_engine_processor_policy_percent(
        &self,
        field: AdaptiveEngineProcessorPolicyField,
    ) -> u32 {
        let values = self
            .settings
            .adaptive_engine
            .processor_policy_values
            .normalized();
        match field {
            AdaptiveEngineProcessorPolicyField::CoreParkingMin => values.core_parking_min,
            AdaptiveEngineProcessorPolicyField::PerformanceMin => values.performance_min,
            AdaptiveEngineProcessorPolicyField::PerformanceMax => values.performance_max,
            AdaptiveEngineProcessorPolicyField::BoostPolicy => values.boost_policy,
        }
    }

    pub(in crate::ui::app) fn set_adaptive_engine_processor_policy_percent(
        &mut self,
        field: AdaptiveEngineProcessorPolicyField,
        value: u64,
    ) {
        let mut values = self
            .settings
            .adaptive_engine
            .processor_policy_values
            .normalized();
        let value = value.min(100) as u32;
        match field {
            AdaptiveEngineProcessorPolicyField::CoreParkingMin => values.core_parking_min = value,
            AdaptiveEngineProcessorPolicyField::PerformanceMin => values.performance_min = value,
            AdaptiveEngineProcessorPolicyField::PerformanceMax => values.performance_max = value,
            AdaptiveEngineProcessorPolicyField::BoostPolicy => values.boost_policy = value,
        }
        self.settings.adaptive_engine.processor_policy_values = values.normalized();
    }

    pub(in crate::ui::app) fn sync_processor_power_slider_states(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (slider, value) in [
            (
                ProcessorPowerSlider::AcCoreParkingMin,
                self.processor_power_ac_core_parking_min,
            ),
            (
                ProcessorPowerSlider::AcPerformanceMin,
                self.processor_power_ac_performance_min,
            ),
            (
                ProcessorPowerSlider::AcPerformanceMax,
                self.processor_power_ac_performance_max,
            ),
            (
                ProcessorPowerSlider::AcBoostPolicy,
                self.processor_power_ac_boost_policy,
            ),
            (
                ProcessorPowerSlider::DcCoreParkingMin,
                self.processor_power_dc_core_parking_min,
            ),
            (
                ProcessorPowerSlider::DcPerformanceMin,
                self.processor_power_dc_performance_min,
            ),
            (
                ProcessorPowerSlider::DcPerformanceMax,
                self.processor_power_dc_performance_max,
            ),
            (
                ProcessorPowerSlider::DcBoostPolicy,
                self.processor_power_dc_boost_policy,
            ),
        ] {
            let input = processor_power_slider_input(&self.inputs, slider);
            let value = value.min(100) as f32;
            input.update(cx, |state, cx| {
                if (state.value().end() - value).abs() > f32::EPSILON {
                    state.set_value(value, window, cx);
                }
            });
        }
    }

    pub(in crate::ui::app) fn refresh_processor_power_values(&mut self) {
        let Some(plan) = self.processor_power_target_plan() else {
            self.status_message = t!("processor_power.no_active_plan").to_string();
            return;
        };
        if self.sync_processor_power_values_from_target_plan(true) {
            self.status_message =
                t!("processor_power.loaded_values", plan = plan.display_name()).to_string();
        }
    }

    pub(in crate::ui::app) fn fill_processor_power_preset(&mut self, preset: ProcessorPowerPreset) {
        let values = ProcessorPowerValues::for_preset(preset);
        self.set_processor_power_values(ProcessorPowerAcDcValues::same(values));
        self.processor_power_dirty = true;
        self.status_message = t!(
            "processor_power.loaded_preset",
            preset = processor_power_preset_label(preset)
        )
        .to_string();
    }

    pub(in crate::ui::app) fn processor_power_matches_preset(
        &self,
        preset: ProcessorPowerPreset,
    ) -> bool {
        let values = ProcessorPowerValues::for_preset(preset);
        self.processor_power_values() == ProcessorPowerAcDcValues::same(values).normalized()
    }

    pub(in crate::ui::app) fn apply_processor_power_custom(&mut self) {
        let Some(plan) = self.processor_power_target_plan() else {
            self.status_message = t!("processor_power.no_active_plan").to_string();
            return;
        };

        let values = self.processor_power_values();
        self.set_processor_power_values(values);

        match self
            .power
            .apply_processor_power_values(&plan.guid, values.normalized())
        {
            Ok(()) => {
                self.processor_power_loaded_plan_guid = Some(plan.guid.clone());
                self.processor_power_dirty = false;
                self.status_message =
                    t!("processor_power.applied_custom", plan = plan.display_name()).to_string();
                self.refresh_active_plan();
            }
            Err(err) => self.status_message = err,
        }
    }
}
