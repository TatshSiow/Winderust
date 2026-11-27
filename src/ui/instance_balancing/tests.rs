use super::*;

#[test]
fn row_status_shows_verified_results_and_treats_invalid_observations_as_unavailable() {
    use ActionLogResult::{Applied, Failed, Skipped};
    use InstanceBalancingDetail::{
        Assigned, Diagnostic, InsufficientCapacity, WaitingForInstances, WaitingForStableCohort,
    };

    assert_eq!(
        rule_status_key(true, Some((Applied, &Assigned(2)))),
        "common.applied"
    );
    assert_eq!(
        rule_status_key(false, Some((Applied, &Assigned(2)))),
        "common.inactive"
    );
    assert_eq!(rule_status_key(true, None), "common.unknown");
    assert_eq!(
        rule_status_key(true, Some((Skipped, &WaitingForInstances))),
        "common.waiting"
    );
    assert_eq!(
        rule_status_key(true, Some((Skipped, &WaitingForStableCohort))),
        "common.waiting"
    );
    assert_eq!(
        rule_status_key(true, Some((Skipped, &InsufficientCapacity))),
        "common.waiting"
    );

    let incomplete = Diagnostic("incomplete observation".into());
    assert_eq!(
        rule_status_key(true, Some((Skipped, &incomplete))),
        "common.unavailable"
    );
    assert_eq!(
        rule_status_key(true, Some((Failed, &incomplete))),
        "common.error"
    );
    assert_eq!(
        rule_status_key(true, Some((Skipped, &Assigned(2)))),
        "common.unknown"
    );
}

fn add(editor: &mut Editor, settings: &mut InstanceBalancingSettings, path: &str) {
    editor.update(settings, Message::Path(path.into()));
    editor.update(settings, Message::Add);
}

#[test]
fn literal_paths_are_preserved_and_invalid_or_equivalent_paths_cannot_be_added() {
    let mut editor = Editor::default();
    let mut settings = InstanceBalancingSettings::default();
    let path = "C:/Apps/É測試.exe";
    add(&mut editor, &mut settings, path);
    assert!(!settings.enabled);
    assert!(settings.rules[0].enabled);
    assert_eq!(settings.rules[0].executable_path, path);
    assert!(editor.path.is_empty());
    for path in [
        "c:\\apps\\é測試.EXE",
        "relative.exe",
        r"C:\Apps\..\app.exe",
        r"\\?\C:\Apps\app.exe",
        "C:\\Apps\\app.exe\0",
    ] {
        add(&mut editor, &mut settings, path);
        assert_eq!(settings.rules.len(), 1, "{path:?}");
        assert!(path_error(&settings, path, None).is_some());
    }
    editor.update(&mut settings, Message::RuleEnabled(0, false));
    add(&mut editor, &mut settings, "c:\\apps\\é測試.EXE");
    assert_eq!(settings.rules.len(), 1);
    editor.update(&mut settings, Message::Enabled(true));
    assert!(settings.enabled);
}

#[test]
fn path_edits_validate_against_other_rules_and_cancel_without_changing_settings() {
    let mut editor = Editor::default();
    let mut settings = InstanceBalancingSettings::default();
    add(&mut editor, &mut settings, r"C:\Apps\first.exe");
    add(&mut editor, &mut settings, r"C:\Apps\second.exe");
    editor.update(&mut settings, Message::RuleEnabled(0, false));
    let original = settings.clone();
    editor.update(&mut settings, Message::Edit(0));
    assert!(path_error(&settings, r"c:/apps/FIRST.exe", Some(0)).is_none());
    for path in ["relative.exe", r"c:/apps/SECOND.exe"] {
        editor.update(&mut settings, Message::Path(path.into()));
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings, original);
        assert!(editor.has_pending_editor());
    }
    let superseded = editor.begin_browse();
    editor.update(&mut settings, Message::Path(r"C:\Other\third.exe".into()));
    assert!(!editor.take_browse(&superseded));
    let dismissed = editor.begin_browse();
    editor.update(&mut settings, Message::CancelRule);
    assert!(!editor.has_pending_editor());
    assert_eq!(settings, original);
    editor.update(&mut settings, Message::Edit(0));
    let current = editor.begin_browse();
    assert!(!editor.take_browse(&dismissed));
    assert!(editor.take_browse(&current));
    assert!(!editor.take_browse(&current));
    editor.update(&mut settings, Message::Path(r"C:\Other\third.exe".into()));
    editor.update(&mut settings, Message::SaveRule);
    assert_eq!(settings.rules[0].executable_path, r"C:\Other\third.exe");
    assert!(!settings.rules[0].enabled);
    assert_eq!(settings.rules[1], original.rules[1]);
    assert!(!editor.has_pending_editor());
}

#[test]
fn removal_cancels_indexed_drafts_and_stale_actions_do_not_change_rules() {
    let mut editor = Editor::default();
    let mut settings = InstanceBalancingSettings::default();
    add(&mut editor, &mut settings, r"C:\Apps\first.exe");
    add(&mut editor, &mut settings, r"C:\Apps\second.exe");
    editor.update(&mut settings, Message::Edit(1));
    editor.update(&mut settings, Message::Remove(0));
    assert!(!editor.has_pending_editor());
    assert_eq!(settings.rules.len(), 1);
    let original = settings.clone();
    editor.update(&mut settings, Message::SaveRule);
    editor.update(&mut settings, Message::Remove(99));
    editor.update(&mut settings, Message::RuleEnabled(99, false));
    editor.update(&mut settings, Message::Edit(99));
    assert_eq!(settings, original);
    assert!(!editor.has_pending_editor());
}
