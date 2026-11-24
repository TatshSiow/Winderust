use std::{
    cell::RefCell,
    sync::{mpsc, Arc, OnceLock},
    time::Duration,
};

use rust_i18n::t;
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_HOTKEY_ALREADY_REGISTERED, HWND, LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F1,
        },
        WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};

use crate::config::{ShowWinderustShortcut, ShowWinderustShortcutKey};

pub(super) const WM_UPDATE_SHOW_SHORTCUT: u32 = WM_APP + 3;
pub(super) const FIRST_REGISTRATION_ID: i32 = 1;
const PREPARE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutError {
    Conflict,
    Unavailable,
    Windows(u32),
}

impl ShortcutError {
    pub(crate) fn localized(self) -> String {
        match self {
            Self::Conflict => t!("settings.show_winderust_shortcut_conflict"),
            Self::Unavailable => t!("settings.show_winderust_shortcut_unavailable"),
            Self::Windows(code) => t!("settings.show_winderust_shortcut_failed", code = code),
        }
        .into_owned()
    }
}

#[derive(Clone, Copy)]
enum Decision {
    Commit,
    Cancel,
}

pub(super) struct Prepare {
    shortcut: Option<ShowWinderustShortcut>,
    decision: Arc<OnceLock<Decision>>,
    reply: mpsc::SyncSender<Result<(), ShortcutError>>,
}

pub(crate) struct PreparedShortcut {
    hwnd: HWND,
    decision: Arc<OnceLock<Decision>>,
    pub(super) shortcut: Option<ShowWinderustShortcut>,
}

impl PreparedShortcut {
    pub(crate) fn commit(self) {
        self.finish(Decision::Commit);
    }

    fn finish(&self, decision: Decision) {
        // Subsequent owner dispatch observes the decision even if the message queue is full.
        if self.decision.set(decision).is_ok() && !wake(self.hwnd) {
            eprintln!("Could not wake the tray window to reconcile the Show Winderust shortcut.");
        }
    }
}

impl Drop for PreparedShortcut {
    fn drop(&mut self) {
        self.finish(Decision::Cancel);
    }
}

pub(super) fn prepare(
    hwnd: HWND,
    inbox: &mpsc::SyncSender<Prepare>,
    shortcut: Option<ShowWinderustShortcut>,
) -> Result<PreparedShortcut, ShortcutError> {
    let (reply, response) = mpsc::sync_channel(1);
    let pending = PreparedShortcut {
        hwnd,
        decision: Arc::default(),
        shortcut,
    };
    inbox
        .try_send(Prepare {
            shortcut,
            decision: Arc::clone(&pending.decision),
            reply,
        })
        .map_err(|_| ShortcutError::Unavailable)?;
    if !wake(hwnd) {
        return Err(ShortcutError::Unavailable);
    }
    response
        .recv_timeout(PREPARE_TIMEOUT)
        .map_err(|_| ShortcutError::Unavailable)??;
    Ok(pending)
}

fn wake(hwnd: HWND) -> bool {
    // SAFETY: this wake posts no pointers. Windows rejects an invalid window handle.
    unsafe { PostMessageW(hwnd, WM_UPDATE_SHOW_SHORTCUT, 0, 0) != 0 }
}

struct Registration {
    hwnd: HWND,
    id: i32,
    shortcut: ShowWinderustShortcut,
    modifiers: u32,
    key: u32,
}

impl Registration {
    fn new(hwnd: HWND, id: i32, shortcut: ShowWinderustShortcut) -> Result<Self, ShortcutError> {
        let (ctrl, alt, shift) = shortcut.modifiers();
        let modifiers = [(ctrl, MOD_CONTROL), (alt, MOD_ALT), (shift, MOD_SHIFT)]
            .into_iter()
            .filter_map(|(enabled, modifier)| enabled.then_some(modifier))
            .fold(0, |combined, modifier| combined | modifier);
        let key = match shortcut.key() {
            ShowWinderustShortcutKey::Character(value) => u32::from(value),
            ShowWinderustShortcutKey::Function(value) => u32::from(VK_F1) + u32::from(value) - 1,
        };
        // SAFETY: the tray thread owns hwnd, and the identifier and chord use Win32's allowed range.
        if unsafe { RegisterHotKey(hwnd, id, modifiers | MOD_NOREPEAT, key) } == 0 {
            // SAFETY: GetLastError immediately follows the failed registration on its caller thread.
            let error = unsafe { GetLastError() };
            return Err(if error == ERROR_HOTKEY_ALREADY_REGISTERED {
                ShortcutError::Conflict
            } else {
                ShortcutError::Windows(error)
            });
        }
        Ok(Self {
            hwnd,
            id,
            shortcut,
            modifiers,
            key,
        })
    }

    fn matches_message(&self, id: WPARAM, chord: LPARAM) -> bool {
        let chord = chord as u32;
        id == self.id as usize
            && (chord & 0xffff & !MOD_NOREPEAT) == self.modifiers
            && (chord >> 16) == self.key
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: the tray thread drops each owned registration before destroying its window.
        if unsafe { UnregisterHotKey(self.hwnd, self.id) } == 0 {
            // SAFETY: GetLastError immediately follows the failed unregistration on its caller thread.
            let error = unsafe { GetLastError() };
            eprintln!("Could not release the Show Winderust shortcut: {error}.");
        }
    }
}

struct StagedShortcut {
    candidate: Option<Registration>,
    decision: Arc<OnceLock<Decision>>,
}

struct Owner {
    inbox: mpsc::Receiver<Prepare>,
    active: Option<Registration>,
    staged: Option<StagedShortcut>,
    next_id: i32,
}

impl Owner {
    fn reconcile(&mut self) {
        let Some(pending) = self
            .staged
            .take_if(|pending| pending.decision.get().is_some())
        else {
            return;
        };
        if matches!(pending.decision.get(), Some(Decision::Commit)) {
            self.active = pending.candidate;
        }
    }

    fn stage(&mut self, hwnd: HWND, request: &Prepare) -> Result<(), ShortcutError> {
        if self.staged.is_some() {
            return Err(ShortcutError::Unavailable);
        }
        if self.active.as_ref().map(|active| active.shortcut) == request.shortcut {
            return Ok(());
        }
        let candidate = request
            .shortcut
            .map(|shortcut| {
                let id = self.next_id;
                self.next_id = if id == 0xbfff {
                    FIRST_REGISTRATION_ID
                } else {
                    id + 1
                };
                Registration::new(hwnd, id, shortcut)
            })
            .transpose()?;
        self.staged = Some(StagedShortcut {
            candidate,
            decision: Arc::clone(&request.decision),
        });
        Ok(())
    }

    fn process(&mut self, hwnd: HWND) {
        self.reconcile();
        while let Ok(request) = self.inbox.try_recv() {
            if request.decision.get().is_none() {
                let result = self.stage(hwnd, &request);
                let _ = request.reply.send(result);
            }
            self.reconcile();
        }
    }
}

thread_local! {
    static OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

pub(super) fn start(inbox: mpsc::Receiver<Prepare>) {
    OWNER.with_borrow_mut(|owner| {
        *owner = Some(Owner {
            inbox,
            active: None,
            staged: None,
            next_id: FIRST_REGISTRATION_ID,
        });
    });
}

pub(super) fn stop() {
    OWNER.with_borrow_mut(|owner| *owner = None);
}

pub(super) fn update(hwnd: HWND) {
    OWNER.with_borrow_mut(|owner| {
        if let Some(owner) = owner {
            owner.process(hwnd);
        }
    });
}

pub(super) fn reconcile_pending() {
    OWNER.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return;
        };
        if let Some(owner) = &mut *slot {
            owner.reconcile();
        }
    });
}

pub(super) fn should_restore(id: WPARAM, chord: LPARAM) -> bool {
    OWNER.with_borrow_mut(|owner| {
        let Some(owner) = owner else {
            return false;
        };
        owner.reconcile();
        owner
            .active
            .as_ref()
            .is_some_and(|active| active.matches_message(id, chord))
    })
}
