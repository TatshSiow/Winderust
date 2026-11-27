use super::*;

fn floor(source: PowerSourceProfile, minimum_percent: u8) -> Option<RestraintCoreParking> {
    Some(RestraintCoreParking {
        source,
        minimum_percent_class0: minimum_percent,
        minimum_percent_class1: 0,
    })
}

#[test]
fn overlay_changes_only_the_selected_slot_and_restores_the_previous_source() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let normal = controller.platform.processor_values.unwrap();

    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 90);
    controller.reconcile_adaptive(request, now).unwrap();
    let ac_overlay = ProcessorPowerSourceValues {
        ac: ProcessorPowerValues {
            core_parking_min: 90,
            ..normal.ac
        },
        ..normal
    };
    assert_eq!(controller.platform.processor_values, Some(ac_overlay));
    assert_eq!(controller.platform.previous_processor_values, Some(normal));

    request.restraint_core_parking = floor(PowerSourceProfile::OnBattery, 95);
    controller.reconcile_adaptive(request, now).unwrap();
    let dc_overlay = ProcessorPowerSourceValues {
        battery: ProcessorPowerValues {
            core_parking_min: 95,
            ..normal.battery
        },
        ..normal
    };
    assert_eq!(controller.platform.processor_values, Some(dc_overlay));
    assert_eq!(
        controller.platform.previous_processor_values,
        Some(ac_overlay)
    );

    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert_eq!(controller.platform.processor_values, Some(normal));
    assert_eq!(
        controller.platform.previous_processor_values,
        Some(dc_overlay)
    );
    assert_eq!(controller.platform.active, "adaptive-1");
    assert!(!controller
        .platform
        .events
        .borrow()
        .iter()
        .any(|event| event == "configure:original"));
}

#[test]
fn floor_at_or_below_current_value_needs_no_write_or_clear_poll() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let normal = controller.platform.processor_values.unwrap();
    let normal_floor = u8::try_from(normal.ac.core_parking_min).unwrap();
    for minimum in [0, normal_floor] {
        controller.platform.events.borrow_mut().clear();
        request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, minimum);
        controller.reconcile_adaptive(request, now).unwrap();
        controller
            .clear_adaptive_restraint_core_parking(now)
            .unwrap();
        assert!(controller.platform.events.borrow().is_empty());
    }
    assert_eq!(controller.platform.processor_values, Some(normal));
}

#[test]
fn unknown_source_retries_partial_first_overlay_and_clear_writes() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let normal = controller.platform.processor_values.unwrap();
    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 95);
    controller.platform.fail_partial_processor_write = true;
    assert!(controller.reconcile_adaptive(request, now).is_err());
    assert_eq!(
        controller
            .platform
            .processor_values
            .unwrap()
            .ac
            .core_parking_min,
        95
    );

    controller.platform.fail_partial_processor_write = true;
    assert!(controller
        .clear_adaptive_restraint_core_parking(now)
        .is_err());
    controller.platform.events.borrow_mut().clear();
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert_eq!(controller.platform.processor_values, Some(normal));
    assert_eq!(controller.platform.previous_processor_values, None);
    assert_eq!(controller.platform.active, "adaptive-1");
    assert!(controller
        .platform
        .events
        .borrow()
        .iter()
        .any(|event| event == "configure:adaptive-1"));

    controller.platform.events.borrow_mut().clear();
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert!(controller.platform.events.borrow().is_empty());
}

#[test]
fn unknown_source_records_read_error_and_keeps_overlay_until_retry() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let normal = controller.platform.processor_values.unwrap();
    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 95);
    controller.reconcile_adaptive(request, now).unwrap();
    let restrained = controller.platform.processor_values;
    controller
        .platform
        .query_failures
        .insert(controller.platform.query_count + 1);
    controller.platform.events.borrow_mut().clear();

    assert!(matches!(
        controller.clear_adaptive_restraint_core_parking(now),
        Err(ActivePlanCheckError::Read(error)) if error == "injected active query failure"
    ));
    assert_eq!(
        controller.status().active_plan_read_error.as_deref(),
        Some("injected active query failure")
    );
    assert_eq!(controller.platform.events.borrow().as_slice(), ["query"]);
    assert_eq!(controller.platform.processor_values, restrained);

    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert!(controller.status().active_plan_read_error.is_none());
    assert_eq!(controller.platform.processor_values, Some(normal));
    assert_eq!(controller.platform.previous_processor_values, restrained);
    assert_eq!(controller.platform.active, "adaptive-1");
}

#[test]
fn overlay_only_mutations_and_partial_repair_preserve_downward_hysteresis() {
    let now = Instant::now();
    let start_downward = now + Duration::from_secs(1);
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    request.profile = AdaptivePowerProfile::BackgroundPressure;
    controller.reconcile_adaptive(request, now).unwrap();
    let original = controller.platform.processor_values.unwrap();

    request.profile = AdaptivePowerProfile::Idle;
    controller
        .reconcile_adaptive(request, start_downward)
        .unwrap();
    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 95);
    let overlay_at = now + Duration::from_secs(2);
    controller.reconcile_adaptive(request, overlay_at).unwrap();
    assert_eq!(
        controller
            .adaptive_plan
            .as_ref()
            .unwrap()
            .lower_demand_since,
        Some(start_downward)
    );

    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 100);
    let update_at = now + Duration::from_secs(3);
    controller.platform.fail_partial_processor_write = true;
    assert!(controller.reconcile_adaptive(request, update_at).is_err());
    controller.reconcile_adaptive(request, update_at).unwrap();
    assert_eq!(
        controller
            .adaptive_plan
            .as_ref()
            .unwrap()
            .lower_demand_since,
        Some(start_downward)
    );
    controller
        .clear_adaptive_restraint_core_parking(update_at)
        .unwrap();
    assert_eq!(controller.platform.processor_values, Some(original));
    assert_eq!(
        controller
            .adaptive_plan
            .as_ref()
            .unwrap()
            .lower_demand_since,
        Some(start_downward)
    );

    request.restraint_core_parking = None;
    assert_eq!(
        controller
            .reconcile_adaptive(request, now + Duration::from_secs(6))
            .unwrap(),
        AdaptivePowerProfile::Idle
    );
}

#[test]
fn normal_tuning_updates_baseline_and_keeps_existing_hysteresis_semantics() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("original"));
    let mut request = adaptive_request();
    request.profile = AdaptivePowerProfile::BackgroundPressure;
    request.restraint_core_parking = floor(PowerSourceProfile::OnBattery, 95);
    controller.reconcile_adaptive(request, now).unwrap();
    request.profile = AdaptivePowerProfile::Idle;
    controller
        .reconcile_adaptive(request, now + Duration::from_secs(1))
        .unwrap();
    request.baseline.performance_min = 71;
    controller
        .reconcile_adaptive(request, now + Duration::from_secs(2))
        .unwrap();
    assert_eq!(controller.platform.previous_processor_values, None);
    assert_eq!(
        controller
            .adaptive_plan
            .as_ref()
            .unwrap()
            .lower_demand_since,
        None
    );

    let (normal, _) = request.calibrated_values(AdaptivePowerProfile::BackgroundPressure);
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert_eq!(controller.platform.processor_values, Some(normal));
}

#[test]
fn clear_without_owned_active_adaptive_guid_cannot_change_ordinary_plan() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("ordinary"));
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert!(controller.platform.events.borrow().is_empty());
    assert!(controller.platform.processor_values.is_none());

    let mut request = adaptive_request();
    request.restraint_core_parking = floor(PowerSourceProfile::PluggedIn, 95);
    controller.reconcile_adaptive(request, now).unwrap();
    controller.platform.active = "ordinary".to_owned();
    controller.platform.events.borrow_mut().clear();
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert_eq!(controller.platform.active, "ordinary");
    assert!(controller.adaptive_plan.is_none());
    assert!(!controller
        .platform
        .events
        .borrow()
        .iter()
        .any(|event| event.starts_with("configure:")));
}

#[test]
fn setup_failure_never_applies_restraint_floor_before_verified_ownership() {
    let now = Instant::now();
    let mut platform = FakePlatform::new("ordinary");
    platform.failure = Some(FailurePoint::Apply);
    let mut controller = PowerPlanController::with_platform(platform);
    let mut request = adaptive_request();
    request.restraint_core_parking = Some(RestraintCoreParking {
        source: PowerSourceProfile::PluggedIn,
        minimum_percent_class0: 95,
        minimum_percent_class1: 90,
    });
    let (normal, _) = request.calibrated_values(request.profile);
    assert!(controller.reconcile_adaptive(request, now).is_err());
    assert_eq!(controller.platform.processor_values, Some(normal));
    assert_eq!(controller.platform.active, "ordinary");
    assert!(!controller
        .platform
        .events
        .borrow()
        .iter()
        .any(|event| event.contains("class1-parking")));
}

#[test]
fn class1_uses_inherited_floors_and_restores_each_source_without_class0_writes() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("ordinary"));
    let normal = controller.platform.class1_core_parking_values;
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let class0 = controller.platform.processor_values;
    assert!(!controller
        .platform
        .events
        .borrow()
        .iter()
        .any(|event| event.contains("class1-parking")));
    controller.platform.events.borrow_mut().clear();

    for (source, minimum_percent_class1, expected) in [
        (PowerSourceProfile::PluggedIn, 10, normal),
        (
            PowerSourceProfile::PluggedIn,
            80,
            Class1CoreParkingValues { ac: 80, ..normal },
        ),
        (
            PowerSourceProfile::OnBattery,
            90,
            Class1CoreParkingValues {
                battery: 90,
                ..normal
            },
        ),
        (PowerSourceProfile::OnBattery, 0, normal),
    ] {
        request.restraint_core_parking = Some(RestraintCoreParking {
            source,
            minimum_percent_class0: 0,
            minimum_percent_class1,
        });
        controller.reconcile_adaptive(request, now).unwrap();
        assert_eq!(controller.platform.class1_core_parking_values, expected);
    }
    assert_eq!(controller.platform.processor_values, class0);
    assert_eq!(
        controller.platform.events.borrow().as_slice(),
        [
            "read-class1-parking:adaptive-1",
            "configure-class1-parking:adaptive-1:80:20",
            "configure-class1-parking:adaptive-1:30:90",
            "configure-class1-parking:adaptive-1:30:20",
        ]
    );
}

#[test]
fn class1_baseline_read_failure_keeps_normal_and_class0_updates_working() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("ordinary"));
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let class1 = controller.platform.class1_core_parking_values;
    controller.platform.fail_class1_core_parking_read = true;
    request.baseline.performance_min = 71;
    request.restraint_core_parking = Some(RestraintCoreParking {
        source: PowerSourceProfile::OnBattery,
        minimum_percent_class0: 95,
        minimum_percent_class1: 90,
    });
    let (_, calibrated) = request.calibrated_values(request.profile);
    assert_eq!(
        controller.reconcile_adaptive(request, now).unwrap_err(),
        "injected class 1 battery baseline read failure"
    );
    assert_eq!(controller.platform.processor_values, Some(calibrated));
    assert_eq!(controller.platform.class1_core_parking_values, class1);
}

#[test]
fn class1_repairs_partial_write_before_new_source_and_clears_even_if_class0_fails() {
    let now = Instant::now();
    let mut controller = PowerPlanController::with_platform(FakePlatform::new("ordinary"));
    let normal = controller.platform.class1_core_parking_values;
    let mut request = adaptive_request();
    controller.reconcile_adaptive(request, now).unwrap();
    let normal_class0 = controller.platform.processor_values;
    request.restraint_core_parking = Some(RestraintCoreParking {
        source: PowerSourceProfile::PluggedIn,
        minimum_percent_class0: 95,
        minimum_percent_class1: 80,
    });
    controller.platform.fail_partial_class1_core_parking_write = true;
    assert!(controller.reconcile_adaptive(request, now).is_err());
    assert_eq!(controller.platform.class1_core_parking_values.ac, 80);

    controller.platform.events.borrow_mut().clear();
    request.restraint_core_parking.as_mut().unwrap().source = PowerSourceProfile::OnBattery;
    controller.reconcile_adaptive(request, now).unwrap();
    assert_eq!(
        controller.platform.events.borrow().as_slice(),
        [
            "configure-class1-parking:adaptive-1:30:20",
            "configure-class1-parking:adaptive-1:30:80",
            "configure:adaptive-1",
        ]
    );

    controller.platform.fail_partial_processor_write = true;
    assert!(controller
        .clear_adaptive_restraint_core_parking(now)
        .is_err());
    assert_eq!(controller.platform.class1_core_parking_values, normal);
    assert_eq!(
        controller
            .platform
            .processor_values
            .unwrap()
            .battery
            .core_parking_min,
        95
    );
    controller
        .clear_adaptive_restraint_core_parking(now)
        .unwrap();
    assert_eq!(controller.platform.processor_values, normal_class0);
}
