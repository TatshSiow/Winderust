use super::*;

#[test]
fn cpu_duration_editor_accepts_only_values_that_round_trip_in_toml() {
    let mut editor = Editor::default();
    let mut settings = SettingsEditor::with_settings(Settings::default());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    let maximum = i64::MAX as u64;
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Duration(0, maximum.to_string()),
    );
    assert!(editor.valid());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    let raw = toml::to_string(settings.global()).unwrap();
    assert_eq!(
        &toml::from_str::<Settings>(&raw).unwrap(),
        settings.global()
    );
    assert_eq!(settings.by_cpu_load.rules[0].duration_seconds, maximum);

    for invalid in [(maximum + 1).to_string(), u64::MAX.to_string(), "-1".into()] {
        editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
        editor.update(
            Kind::CpuLoad,
            &mut settings,
            &[],
            Message::Duration(0, invalid),
        );
        assert!(!editor.valid());
        assert_eq!(
            editor.draft.as_ref().unwrap().by_cpu_load.rules[0].duration_seconds,
            maximum
        );
        editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
        assert_eq!(settings.by_cpu_load.rules[0].duration_seconds, maximum);
        editor.update(Kind::CpuLoad, &mut settings, &[], Message::Cancel);
    }
}

#[test]
fn time_picker_preserves_typed_drafts_and_only_saves_submitted_times() {
    let mut editor = Editor::default();
    let mut settings = Settings::default();
    settings.by_time.rules.clear();
    let mut settings = SettingsEditor::with_settings(settings);
    let update = |editor: &mut Editor, settings: &mut SettingsEditor, message| {
        editor.update(Kind::Time, settings, &[], message);
    };
    update(&mut editor, &mut settings, Message::Add);
    update(&mut editor, &mut settings, Message::Start(0, "2:".into()));
    assert!(!editor.valid());
    update(&mut editor, &mut settings, Message::OpenTimePicker(true));
    update(&mut editor, &mut settings, Message::CancelTimePicker);
    assert!(!editor.valid());
    assert_eq!(
        editor.draft.as_ref().unwrap().by_time.rules[0]
            .start_time
            .to_string(),
        "22:00"
    );
    let time = chrono::NaiveTime::from_hms_opt(0, 5, 0).unwrap();
    update(&mut editor, &mut settings, Message::TimePicked(true, time));
    assert!(
        !editor.valid(),
        "dismissed picker must not replace typed text"
    );
    update(&mut editor, &mut settings, Message::OpenTimePicker(true));
    assert_eq!(editor.picker_time.hour, "22");
    update(&mut editor, &mut settings, Message::TimePicked(true, time));
    assert!(
        editor.valid(),
        "drafts: {:?}, ids: {:?}",
        editor.inputs,
        editor.ids
    );
    assert!(settings.by_time.rules.is_empty());
    assert_eq!(
        editor.draft.as_ref().unwrap().by_time.rules[0]
            .start_time
            .to_string(),
        "00:05"
    );
    update(&mut editor, &mut settings, Message::OpenTimePicker(false));
    let time = chrono::NaiveTime::from_hms_opt(23, 59, 0).unwrap();
    update(&mut editor, &mut settings, Message::TimePicked(false, time));
    update(&mut editor, &mut settings, Message::Save);
    assert_eq!(settings.by_time.rules[0].start_time.to_string(), "00:05");
    assert_eq!(settings.by_time.rules[0].end_time.to_string(), "23:59");
    update(&mut editor, &mut settings, Message::Edit(0));
    update(&mut editor, &mut settings, Message::OpenTimePicker(true));
    editor.dismiss_picker();
    update(&mut editor, &mut settings, Message::TimePicked(true, time));
    update(&mut editor, &mut settings, Message::Cancel);
    assert_eq!(settings.by_time.rules[0].start_time.to_string(), "00:05");
}

#[test]
fn by_time_dialog_stays_centered_with_picker_open_or_closed() {
    use iced::advanced::{layout, widget::Tree, Layout};
    use iced::{Rectangle, Size};
    fn find_frame(layout: Layout<'_>, size: Size) -> Option<Rectangle> {
        if layout.bounds().size() == size {
            return Some(layout.bounds());
        }
        layout.children().find_map(|child| find_frame(child, size))
    }
    let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
    let mut editor = Editor::default();
    let mut settings = SettingsEditor::with_settings(Settings::default());
    editor.update(Kind::Time, &mut settings, &[], Message::Edit(0));
    for (width, height) in [(900.0f32, 620.0f32), (1400.0, 1000.0)] {
        for open in [false, true] {
            if open {
                editor.update(
                    Kind::Time,
                    &mut settings,
                    &[],
                    Message::OpenTimePicker(true),
                );
            } else {
                editor.dismiss_picker();
            }
            let mut view: Element<'_, Message> = container(editor.modal(Kind::Time, &[]).unwrap())
                .padding(16)
                .center_x(Fill)
                .center_y(Fill)
                .into();
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, height)),
            );
            let frame = find_frame(
                Layout::new(&node),
                Size::new((width - 32.0).min(800.0), (height - 32.0).min(760.0)),
            )
            .expect("visible dialog frame");
            assert_eq!(frame.center(), iced::Point::new(width / 2.0, height / 2.0));
        }
    }
}

#[test]
fn rule_tables_align_columns_and_keep_controls_working_at_both_widths() {
    use iced::advanced::{layout, widget::Tree, Layout, Shell};
    use iced::{mouse, Event, Rectangle, Size};
    fn find_rows<'a>(layout: Layout<'a>, rows: &mut Vec<Layout<'a>>) {
        let cells: Vec<_> = layout.children().collect();
        if cells.len() == 4
            && cells[0].bounds().width == widgets::RULE_ENABLE_WIDTH as f32
            && cells[1].bounds().width == widgets::RULE_STATUS_WIDTH as f32
            && cells[3].bounds().width == widgets::RULE_ACTIONS_WIDTH as f32
        {
            rows.push(layout);
        } else {
            for child in cells {
                find_rows(child, rows);
            }
        }
    }
    let editor = Editor::default();
    let mut settings = Settings::default();
    settings.by_time.rules = vec![
        ByTimeRule {
            enabled: true,
            name: "Morning profile".into(),
            ..Default::default()
        },
        ByTimeRule {
            enabled: true,
            name: "A longer rule name that should stay inside its column".into(),
            ..Default::default()
        },
    ];
    settings.by_cpu_load.rules = ["CPU rule", "Second CPU rule"]
        .into_iter()
        .map(|name| ByCpuLoadRule {
            enabled: true,
            name: name.into(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(50).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 10,
            stability: None,
            power_plan_guid: Some("plan".into()),
            else_enabled: false,
            else_power_plan_guid: None,
        })
        .collect();
    let status = crate::control::power_plan::PowerPlanStatus::default();
    for width in [430, 1000] {
        for kind in [Kind::Time, Kind::CpuLoad] {
            for count in [0, 2] {
                let mut settings = settings.clone();
                settings.by_time.rules.truncate(count);
                settings.by_cpu_load.rules.truncate(count);
                let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
                let mut view = editor.view(kind, &settings, None, &status, PageScroll::default());
                let mut tree = Tree::new(&view);
                let bounds = Rectangle::with_size(Size::new(width as f32, 400.0));
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, bounds.size()),
                );
                assert_eq!(node.bounds().width, width as f32);
                let mut rows = Vec::new();
                find_rows(Layout::new(&node), &mut rows);
                assert_eq!(rows.len(), count + 1, "header and rule rows");
                let expected_width = (width as f32 - crate::ui::scrolling::SCROLLBAR_GUTTER)
                    .max(600.0)
                    - 2.0 * widgets::CARD_PADDING as f32;
                for row in &rows {
                    assert_eq!(row.bounds().width, expected_width);
                    for (header, cell) in rows[0].children().zip(row.children()) {
                        assert_eq!(header.bounds().x, cell.bounds().x);
                        assert_eq!(header.bounds().width, cell.bounds().width);
                    }
                }
                view.as_widget_mut().update(
                    &mut tree,
                    &Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now(),
                    )),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut Vec::new()),
                    &bounds,
                );
                if count == 0 {
                    continue;
                }
                let first_row: Vec<_> = rows[1].children().collect();
                let second_row: Vec<_> = rows[2].children().collect();
                let toggle = first_row[0].children().next().unwrap().bounds().center();
                let edit = second_row[3]
                    .children()
                    .next()
                    .unwrap()
                    .children()
                    .next()
                    .unwrap()
                    .bounds()
                    .center();
                let remove = first_row[3]
                    .children()
                    .next()
                    .unwrap()
                    .children()
                    .last()
                    .unwrap()
                    .bounds()
                    .center();
                for (cursor, action) in [(toggle, 0), (edit, 1), (remove, 2)] {
                    if action != 0 && width == 430 {
                        continue;
                    }
                    assert!(
                        bounds.contains(cursor),
                        "visible rule control: {kind:?}/{width}/{action}"
                    );
                    let mut messages = Vec::new();
                    for event in [
                        mouse::Event::ButtonPressed(mouse::Button::Left),
                        mouse::Event::ButtonReleased(mouse::Button::Left),
                    ] {
                        view.as_widget_mut().update(
                            &mut tree,
                            &Event::Mouse(event),
                            Layout::new(&node),
                            mouse::Cursor::Available(cursor),
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut messages),
                            &bounds,
                        );
                    }
                    assert!(
                        matches!(
                            (action, messages.as_slice()),
                            (0, [Message::RuleEnabled(0, false)])
                                | (1, [Message::Edit(1)])
                                | (2, [Message::Remove(0)])
                        ),
                        "unexpected rule action: {messages:?}"
                    );
                }
            }
        }
    }
}
#[test]
fn runtime_status_follows_only_an_unambiguous_unchanged_draft_rule() {
    let rules = [1, 2, 3];
    for (published, draft, index, expected) in [
        (Some(rules.as_slice()), &[2, 3][..], 1, Some(0)),
        (Some(rules.as_slice()), &[1, 4, 3][..], 1, None),
        (Some(rules.as_slice()), &[1, 3][..], 1, None),
        (Some(rules.as_slice()), &[2, 2, 3][..], 1, None),
        (Some(&[2, 2, 3][..]), &[2, 3][..], 1, None),
        (Some(&[2, 2, 3][..]), &[2, 2, 3][..], 1, Some(1)),
        (Some(rules.as_slice()), rules.as_slice(), 3, None),
        (None, rules.as_slice(), 1, None),
    ] {
        assert_eq!(
            runtime_rule_index_in_draft(published, draft, Some(index)),
            expected
        );
    }
}

#[test]
fn cpu_rule_status_tracks_selected_else_plan_and_failed_switches() {
    use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};
    let mut rule = ByCpuLoadRule {
        enabled: true,
        power_plan_guid: Some("main".into()),
        else_enabled: true,
        else_power_plan_guid: Some("else".into()),
        name: "Rule".into(),
        comparison: CpuUsageComparison::AtOrAbove,
        threshold_percent: CpuLoadPercent::new(75).unwrap(),
        upper_threshold_percent: None,
        duration_seconds: 0,
        stability: None,
    };
    let mut status = PowerPlanStatus {
        owner: Some(PowerPlanOwner::OrdinaryAutomation),
        decision_state: Some(crate::rules::DecisionState::ByCpuLoad),
        target_guid: Some("else".into()),
        current_guid: Some("ELSE".into()),
        ..Default::default()
    };
    assert_eq!(
        rule_status(RuleRef::Cpu(&rule), true, true, &status),
        "common.applied"
    );
    assert_eq!(
        rule_status(RuleRef::Cpu(&rule), true, false, &status),
        "common.waiting"
    );
    assert_eq!(
        rule_status(RuleRef::Cpu(&rule), false, true, &status),
        "common.inactive"
    );
    status.current_guid = Some("other".into());
    assert_eq!(
        rule_status(RuleRef::Cpu(&rule), true, true, &status),
        "common.waiting"
    );
    rule.power_plan_guid = None;
    rule.else_enabled = false;
    assert_eq!(
        rule_status(RuleRef::Cpu(&rule), true, true, &status),
        "common.unknown"
    );
}

#[test]
fn time_rule_status_requires_enabled_and_confirmed_runtime_plan() {
    use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};
    let rule = ByTimeRule {
        enabled: true,
        days: vec![WeekdaySetting::from_chrono(chrono::Weekday::Fri)],
        start_time: "22:00".parse().unwrap(),
        end_time: "08:00".parse().unwrap(),
        power_plan_guid: Some("plan".into()),
        ..Default::default()
    };
    let mut status = PowerPlanStatus {
        owner: Some(PowerPlanOwner::OrdinaryAutomation),
        decision_state: Some(crate::rules::DecisionState::ByTime),
        target_guid: Some("plan".into()),
        current_guid: Some("other".into()),
        ..Default::default()
    };
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, true, &status),
        "common.waiting"
    );
    status.apply_failed = true;
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, true, &status),
        "common.error"
    );
    status.apply_failed = false;
    status.current_guid = Some("PLAN".into());
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, true, &status),
        "common.applied"
    );
    assert_eq!(
        rule_status(RuleRef::Time(&rule), false, true, &status),
        "common.inactive"
    );
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, false, &status),
        "common.waiting"
    );
    status.owner = Some(PowerPlanOwner::AdaptiveProcessorPower);
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, true, &status),
        "common.waiting"
    );
    status.owner = Some(PowerPlanOwner::OrdinaryAutomation);
    status.decision_state = Some(crate::rules::DecisionState::Disabled);
    assert_eq!(
        rule_status(RuleRef::Time(&rule), true, true, &status),
        "common.waiting"
    );
}

#[test]
fn rule_modal_saves_and_cancels_without_mutating_live_rules() {
    for kind in [Kind::Time, Kind::CpuLoad] {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings.by_time.enabled = true;
        settings.by_cpu_load.enabled = true;
        let before = settings.clone();
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        editor.update(kind, &mut settings, &[], Message::Add);
        assert!(editor.has_pending_editor());
        assert_eq!(settings.global(), &before);
        editor.update(kind, &mut settings, &[], Message::Cancel);
        assert!(!editor.has_pending_editor());
        assert_eq!(settings.global(), &before);
        editor.update(kind, &mut settings, &[], Message::Add);
        assert!(editor.has_pending_editor());
        let index = editor.editing;
        editor.update(
            kind,
            &mut settings,
            &[],
            Message::Name(index, "New rule".into()),
        );
        editor.update(kind, &mut settings, &[], Message::Save);
        assert!(!editor.has_pending_editor());
        assert_ne!(settings.global(), &before);
        assert_eq!(settings.global().by_time, before.by_time);
        assert_eq!(settings.global().by_cpu_load, before.by_cpu_load);
        let saved = settings.global().clone();
        editor.update(kind, &mut settings, &[], Message::Edit(index));
        editor.update(
            kind,
            &mut settings,
            &[],
            Message::Name(index, "Changed".into()),
        );
        assert_eq!(settings.global(), &saved);
        editor.update(kind, &mut settings, &[], Message::Cancel);
        assert!(!editor.has_pending_editor());
        assert_eq!(settings.global(), &saved);
    }
}

#[test]
fn drafts_do_not_publish_invalid_values_and_confirmed_removal_is_immediate() {
    let mut e = Editor::default();
    let mut s = SettingsEditor::with_settings(Settings::default());
    let old = s.by_time.rules[0].start_time;
    e.update(Kind::Time, &mut s, &[], Message::Start(0, "25:00".into()));
    assert!(!e.valid());
    assert_eq!(s.by_time.rules[0].start_time, old);
    e.update(Kind::Time, &mut s, &[], Message::Start(0, "23:30".into()));
    assert!(e.valid());
    e.update(Kind::Time, &mut s, &[], Message::Remove(0));
    assert!(s.by_time.rules.is_empty());
}
#[test]
fn cpu_rules_keep_per_rule_plans_and_typed_threshold_limits() {
    let mut e = Editor::default();
    let mut s = Settings::default();
    s.by_cpu_load.enabled = true;
    s.by_cpu_load.rules.clear();
    let mut s = SettingsEditor::with_settings(s);
    e.update(Kind::CpuLoad, &mut s, &[], Message::Add);
    e.update(Kind::CpuLoad, &mut s, &[], Message::Save);
    e.update(
        Kind::CpuLoad,
        &mut s,
        &[],
        Message::Threshold(0, "101".into()),
    );
    assert_eq!(s.by_cpu_load.rules[0].threshold_percent.get(), 20);
    assert!(!e.valid());
    e.update(
        Kind::CpuLoad,
        &mut s,
        &[],
        Message::Threshold(0, "75".into()),
    );
    assert!(e.valid());
    e.update(
        Kind::CpuLoad,
        &mut s,
        &[],
        Message::Plan(0, Some("main".into())),
    );
    e.update(
        Kind::CpuLoad,
        &mut s,
        &[],
        Message::ElsePlan(0, Some("else".into())),
    );
    assert_eq!(
        s.by_cpu_load.rules[0].power_plan_guid.as_deref(),
        Some("main")
    );
    assert_eq!(
        s.by_cpu_load.rules[0].else_power_plan_guid.as_deref(),
        Some("else")
    );
}

#[test]
fn cpu_range_edits_remain_transactional_until_both_bounds_are_valid() {
    let mut editor = Editor::default();
    let mut settings = Settings::default();
    settings.by_cpu_load.rules[0].comparison = CpuUsageComparison::Between;
    settings.by_cpu_load.rules[0].upper_threshold_percent = Some(CpuLoadPercent::new(60).unwrap());
    let before = settings.clone();
    let mut settings = SettingsEditor::with_settings(settings);
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Threshold(0, "80".into()),
    );
    assert!(!editor.valid());
    assert_eq!(settings.global(), &before);
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    assert!(editor.has_pending_editor());
    assert_eq!(settings.global(), &before);
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Upper(0, "".into()),
    );
    assert!(!editor.valid());
    assert_eq!(editor.inputs[&(editor.ids[0], Field::Upper)], "");
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Upper(0, "90".into()),
    );
    assert!(editor.valid());
    assert_eq!(settings.global(), &before);
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    assert!(!editor.has_pending_editor());
    assert_eq!(settings.by_cpu_load.rules[0].threshold_percent.get(), 80);
    assert_eq!(
        settings.by_cpu_load.rules[0]
            .upper_threshold_percent
            .unwrap()
            .get(),
        90
    );

    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Upper(0, "20".into()),
    );
    assert!(!editor.valid());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Cancel);
    assert!(editor.valid());
    assert_eq!(
        settings.by_cpu_load.rules[0]
            .upper_threshold_percent
            .unwrap()
            .get(),
        90
    );
}

#[test]
fn cpu_stability_edits_preserve_invalid_drafts_until_saved_or_cancelled() {
    let mut editor = Editor::default();
    let mut settings = SettingsEditor::with_settings(Settings::default());
    let before = settings.global().clone();
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Stability(0, true),
    );
    let stability = editor.draft.as_ref().unwrap().by_cpu_load.rules[0]
        .stability
        .as_ref()
        .unwrap();
    assert_eq!(stability.exit_threshold_percent.get(), 15);
    assert_eq!(stability.minimum_hold_seconds, 0);
    assert!(editor.valid());

    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::ExitThreshold(0, "10".into()),
    );
    assert!(!editor.valid());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    assert_eq!(settings.global(), &before);
    assert!(editor.has_pending_editor());
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::ExitThreshold(0, "".into()),
    );
    assert_eq!(editor.inputs[&(editor.ids[0], Field::ExitThreshold)], "");
    assert!(!editor.valid());
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::ExitThreshold(0, "25".into()),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::MinimumHold(0, u64::MAX.to_string()),
    );
    assert_eq!(
        editor.inputs[&(editor.ids[0], Field::MinimumHold)],
        u64::MAX.to_string()
    );
    assert!(!editor.valid());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    assert_eq!(settings.global(), &before);
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::MinimumHold(0, "90000".into()),
    );
    assert!(editor.valid());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    let saved = settings.global().clone();
    let stability = settings.by_cpu_load.rules[0].stability.as_ref().unwrap();
    assert_eq!(stability.exit_threshold_percent.get(), 25);
    assert_eq!(stability.minimum_hold_seconds, 90_000);
    assert_eq!(
        &toml::from_str::<Settings>(&toml::to_string(settings.global()).unwrap()).unwrap(),
        settings.global()
    );

    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Stability(0, false),
    );
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Cancel);
    assert_eq!(settings.global(), &saved);
}

#[test]
fn cpu_stability_hides_no_invalid_drafts_after_toggle_or_comparison_changes() {
    let mut editor = Editor::default();
    let mut settings = SettingsEditor::with_settings(Settings::default());
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Comparison(0, CpuUsageComparison::Between),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Upper(0, "60".into()),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Stability(0, true),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::MinimumHold(0, "30".into()),
    );
    for message in [
        Message::Upper(0, "".into()),
        Message::ExitThreshold(0, "".into()),
        Message::ExitUpper(0, "".into()),
    ] {
        editor.update(Kind::CpuLoad, &mut settings, &[], message);
    }
    assert!(!editor.valid());
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Comparison(0, CpuUsageComparison::AtOrAbove),
    );
    assert!(editor.valid());
    let stability = editor.draft.as_ref().unwrap().by_cpu_load.rules[0]
        .stability
        .as_ref()
        .unwrap();
    assert_eq!(stability.exit_threshold_percent.get(), 15);
    assert_eq!(stability.exit_upper_threshold_percent, None);
    assert_eq!(stability.minimum_hold_seconds, 30);

    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Comparison(0, CpuUsageComparison::Between),
    );
    let stability = editor.draft.as_ref().unwrap().by_cpu_load.rules[0]
        .stability
        .as_ref()
        .unwrap();
    assert_eq!(stability.exit_upper_threshold_percent.unwrap().get(), 60);
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::ExitUpper(0, "101".into()),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::MinimumHold(0, "-1".into()),
    );
    assert!(!editor.valid());
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Stability(0, false),
    );
    assert!(editor.valid());
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Stability(0, true),
    );
    assert_eq!(
        editor.draft.as_ref().unwrap().by_cpu_load.rules[0]
            .stability
            .as_ref()
            .unwrap()
            .minimum_hold_seconds,
        0
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::MinimumHold(0, "-1".into()),
    );
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Comparison(0, CpuUsageComparison::Else),
    );
    assert!(editor.valid());
    assert!(editor.draft.as_ref().unwrap().by_cpu_load.rules[0]
        .stability
        .is_none());
}

#[test]
fn long_cpu_durations_stay_visible_and_edit_transactionally() {
    use iced::advanced::{
        layout,
        widget::{operation::TextInput, Id, Operation, Tree},
        Layout, Shell,
    };
    use iced::{keyboard, mouse, Event, Rectangle, Size};

    #[derive(Default)]
    struct InputBounds(Option<Rectangle>);

    impl Operation for InputBounds {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }

        fn text_input(&mut self, _: Option<&Id>, bounds: Rectangle, _: &mut dyn TextInput) {
            self.0 = Some(bounds);
        }
    }

    let mut settings = Settings::default();
    settings.by_cpu_load.rules[0].duration_seconds = 90_000;
    let settings = toml::from_str(&toml::to_string(&settings).unwrap()).unwrap();
    let mut settings = SettingsEditor::with_settings(settings);
    let mut editor = Editor::default();
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    // Typing at the end of the actual numeric control must retain its full imported value.
    let mut view = editor.number(
        (editor.ids[0], 0),
        Field::Duration,
        "by_cpu_load.duration",
        editor.draft.as_ref().unwrap().by_cpu_load.rules[0].duration_seconds,
        Message::Duration,
    );
    let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
    let mut tree = Tree::new(&view);
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(700.0, 100.0)),
    );
    let mut input_bounds = InputBounds::default();
    view.as_widget_mut()
        .operate(&mut tree, Layout::new(&node), &renderer, &mut input_bounds);
    let cursor = mouse::Cursor::Available(input_bounds.0.unwrap().center());
    let key = |key: keyboard::Key, value| {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: value,
            repeat: false,
        })
    };
    let mut messages = Vec::new();
    for event in [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        key(keyboard::Key::Named(keyboard::key::Named::End), None),
        key(keyboard::Key::Character("0".into()), Some("0".into())),
    ] {
        view.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            cursor,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &node.bounds(),
        );
    }
    assert!(matches!(messages.as_slice(), [Message::Duration(0, value)] if value == "900000"));
    drop(view);
    let edited_duration = u64::from(u32::MAX) + 1;
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Duration(0, edited_duration.to_string()),
    );
    assert!(editor.valid());
    assert_eq!(settings.by_cpu_load.rules[0].duration_seconds, 90_000);
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Save);
    assert_eq!(
        settings.by_cpu_load.rules[0].duration_seconds,
        edited_duration
    );
    assert_eq!(
        toml::from_str::<Settings>(&toml::to_string(settings.global()).unwrap())
            .unwrap()
            .by_cpu_load
            .rules[0]
            .duration_seconds,
        edited_duration
    );
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Edit(0));
    editor.update(
        Kind::CpuLoad,
        &mut settings,
        &[],
        Message::Duration(0, "30".into()),
    );
    editor.update(Kind::CpuLoad, &mut settings, &[], Message::Cancel);
    assert_eq!(
        settings.by_cpu_load.rules[0].duration_seconds,
        edited_duration
    );
}
