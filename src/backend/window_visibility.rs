use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

type VisibilityCallback = Arc<dyn Fn(bool) + Send + Sync>;

#[derive(Default)]
struct VisibilityState {
    hidden: AtomicBool,
    callback: Mutex<Option<VisibilityCallback>>,
}

impl VisibilityState {
    fn set_hidden(&self, hidden: bool) -> bool {
        if self.hidden.swap(hidden, Ordering::Relaxed) == hidden {
            return false;
        }
        let callback = self
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(callback) = callback {
            if catch_unwind(AssertUnwindSafe(|| callback(hidden))).is_err() {
                eprintln!("Window visibility callback panicked.");
            }
        }
        true
    }

    fn register(&self, callback: VisibilityCallback) {
        *self
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(callback);
    }

    fn unregister(&self, callback: &VisibilityCallback) {
        let mut slot = self
            .callback
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, callback))
        {
            *slot = None;
        }
    }
}

static VISIBILITY: VisibilityState = VisibilityState {
    hidden: AtomicBool::new(false),
    callback: Mutex::new(None),
};

pub(crate) fn is_hidden() -> bool {
    VISIBILITY.hidden.load(Ordering::Relaxed)
}

/// Publishes application visibility and reports whether the state changed.
pub(crate) fn set_hidden(hidden: bool) -> bool {
    VISIBILITY.set_hidden(hidden)
}

pub(crate) struct VisibilityWatcher {
    callback: VisibilityCallback,
}

impl VisibilityWatcher {
    pub(crate) fn start(callback: VisibilityCallback) -> Self {
        VISIBILITY.register(Arc::clone(&callback));
        Self { callback }
    }
}

impl Drop for VisibilityWatcher {
    fn drop(&mut self) {
        VISIBILITY.unregister(&self.callback);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn visibility_transitions_notify_once_and_unregistered_callbacks_stop() {
        let state = VisibilityState::default();
        let changes = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&changes);
        let callback: VisibilityCallback = Arc::new(move |hidden| {
            received.lock().unwrap().push(hidden);
        });
        state.register(Arc::clone(&callback));

        assert!(!state.set_hidden(false));
        assert!(state.set_hidden(true));
        assert!(!state.set_hidden(true));
        assert!(state.set_hidden(false));
        state.unregister(&callback);
        assert!(state.set_hidden(true));
        assert_eq!(*changes.lock().unwrap(), [true, false]);
    }

    #[test]
    fn old_watcher_cannot_unregister_its_replacement() {
        let state = VisibilityState::default();
        let old: VisibilityCallback = Arc::new(|_| panic!("replaced callback must not run"));
        let calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::clone(&calls);
        let replacement: VisibilityCallback = Arc::new(move |_| {
            received.fetch_add(1, Ordering::Relaxed);
        });
        state.register(Arc::clone(&old));
        state.register(replacement);
        state.unregister(&old);

        assert!(state.set_hidden(true));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn panicking_callback_does_not_escape_or_poison_registration() {
        let state = VisibilityState::default();
        state.register(Arc::new(|_| panic!("callback failure")));
        assert!(state.set_hidden(true));
        assert!(state.hidden.load(Ordering::Relaxed));

        let calls = Arc::new(AtomicUsize::new(0));
        let received = Arc::clone(&calls);
        state.register(Arc::new(move |_| {
            received.fetch_add(1, Ordering::Relaxed);
        }));
        assert!(state.set_hidden(false));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
