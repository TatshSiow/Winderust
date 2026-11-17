use std::{cell::RefCell, rc::Rc};

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Create,
    Set(RequestKind),
    Clear(RequestKind),
    Close,
}

#[derive(Default)]
struct State {
    events: Vec<Event>,
    fail_next: Option<&'static str>,
}

struct FakeHandle(Rc<RefCell<State>>);

impl Drop for FakeHandle {
    fn drop(&mut self) {
        self.0.borrow_mut().events.push(Event::Close);
    }
}

struct FakePlatform(Rc<RefCell<State>>);

impl FakePlatform {
    fn call(&self, event: Event, operation: &'static str) -> Result<(), SleepPreventionError> {
        let mut state = self.0.borrow_mut();
        state.events.push(event);
        if state.fail_next == Some(operation) {
            state.fail_next = None;
            Err(SleepPreventionError { operation, code: 5 })
        } else {
            Ok(())
        }
    }
}

impl SleepPreventionPlatform for FakePlatform {
    type Handle = FakeHandle;

    fn create(&self, _reason: &str) -> Result<Self::Handle, SleepPreventionError> {
        self.call(Event::Create, "create")?;
        Ok(FakeHandle(Rc::clone(&self.0)))
    }

    fn set(&self, _handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError> {
        self.call(Event::Set(kind), "set")
    }

    fn clear(&self, _handle: &Self::Handle, kind: RequestKind) -> Result<(), SleepPreventionError> {
        self.call(Event::Clear(kind), "clear")
    }
}

fn controller() -> (SleepPreventionController<FakePlatform>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State::default()));
    (
        SleepPreventionController::new(FakePlatform(Rc::clone(&state))),
        state,
    )
}

#[test]
fn polling_reason_changes_and_display_changes_do_not_renew_the_system_request() {
    let (mut controller, state) = controller();
    for (mode, reason) in [
        (SleepPreventionMode::System, "first"),
        (SleepPreventionMode::System, "changed reason"),
        (SleepPreventionMode::SystemAndDisplay, "display"),
        (SleepPreventionMode::SystemAndDisplay, "display"),
        (SleepPreventionMode::System, "system"),
    ] {
        controller.reconcile(mode, reason).unwrap();
    }
    assert_eq!(
        state.borrow().events,
        vec![
            Event::Create,
            Event::Set(RequestKind::System),
            Event::Set(RequestKind::Display),
            Event::Clear(RequestKind::Display),
        ]
    );
    assert_eq!(
        controller.requests(),
        SleepPreventionRequests {
            system: true,
            display: false,
        }
    );
}

#[test]
fn initial_failures_leave_no_request_and_close_any_created_object() {
    for (operation, expected) in [
        ("create", vec![Event::Create]),
        (
            "set",
            vec![Event::Create, Event::Set(RequestKind::System), Event::Close],
        ),
    ] {
        let (mut controller, state) = controller();
        state.borrow_mut().fail_next = Some(operation);
        let error = controller
            .reconcile(SleepPreventionMode::System, "test")
            .unwrap_err();
        assert_eq!(error.operation, operation);
        assert_eq!(controller.requests(), SleepPreventionRequests::default());
        assert_eq!(state.borrow().events, expected);
    }
}

#[test]
fn failed_display_changes_preserve_accepted_requests_for_retry() {
    let (mut controller, state) = controller();
    controller
        .reconcile(SleepPreventionMode::System, "test")
        .unwrap();
    state.borrow_mut().fail_next = Some("set");
    assert!(controller
        .reconcile(SleepPreventionMode::SystemAndDisplay, "test")
        .is_err());
    assert!(controller.requests().system);
    assert!(!controller.requests().display);
    controller
        .reconcile(SleepPreventionMode::SystemAndDisplay, "test")
        .unwrap();
    state.borrow_mut().fail_next = Some("clear");
    assert!(controller
        .reconcile(SleepPreventionMode::System, "test")
        .is_err());
    assert!(controller.requests().system && controller.requests().display);
    controller
        .reconcile(SleepPreventionMode::System, "test")
        .unwrap();
    assert!(!controller.requests().display);
}

#[test]
fn shutdown_closes_the_object_even_if_windows_has_terminated_its_counts() {
    let (mut controller, state) = controller();
    controller
        .reconcile(SleepPreventionMode::SystemAndDisplay, "test")
        .unwrap();
    state.borrow_mut().fail_next = Some("clear");
    assert_eq!(controller.shutdown().unwrap_err().code, 5);
    assert_eq!(controller.requests(), SleepPreventionRequests::default());
    assert_eq!(
        state.borrow().events,
        vec![
            Event::Create,
            Event::Set(RequestKind::System),
            Event::Set(RequestKind::Display),
            Event::Clear(RequestKind::Display),
            Event::Clear(RequestKind::System),
            Event::Close,
        ]
    );
    let calls = state.borrow().events.len();
    controller.shutdown().unwrap();
    drop(controller);
    assert_eq!(state.borrow().events.len(), calls);
}

#[test]
#[ignore = "Briefly creates native system/display requests; run alone on Windows"]
fn native_requests_accept_transitions_and_release_on_shutdown() -> Result<(), SleepPreventionError>
{
    let mut controller = SleepPreventionController::default();
    for (mode, display) in [
        (SleepPreventionMode::System, false),
        (SleepPreventionMode::SystemAndDisplay, true),
        (SleepPreventionMode::SystemAndDisplay, true),
        (SleepPreventionMode::System, false),
    ] {
        controller.reconcile(mode, "Winderust Sleep Prevention native smoke test")?;
        assert_eq!(
            controller.requests(),
            SleepPreventionRequests {
                system: true,
                display,
            }
        );
    }
    controller.reconcile(SleepPreventionMode::Off, "test complete")?;
    assert_eq!(controller.requests(), SleepPreventionRequests::default());
    controller.shutdown()?;
    assert_eq!(controller.requests(), SleepPreventionRequests::default());
    Ok(())
}
