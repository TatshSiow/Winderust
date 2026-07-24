use std::collections::BTreeSet;

use windows::{
    core::{IUnknown, Interface, HRESULT},
    Win32::{
        Media::Audio::{
            eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
            IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
        },
    },
};

pub fn active_audio_process_ids() -> Result<BTreeSet<u32>, String> {
    let _com = ComApartment::initialize()?;
    let mut process_ids = BTreeSet::new();

    // SAFETY: COM is initialized for this thread by ComApartment and the requested class and
    // interface identifiers are supplied by the windows crate.
    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL)
            .map_err(|err| format!("Failed to create audio device enumerator: {err}."))?
    };
    // SAFETY: enumerator is a live COM interface and the method owns its returned interface.
    let devices = unsafe {
        enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|err| format!("Failed to enumerate audio output devices: {err}."))?
    };
    // SAFETY: devices is a live COM collection interface.
    let device_count = unsafe {
        devices
            .GetCount()
            .map_err(|err| format!("Failed to count audio output devices: {err}."))?
    };

    for device_index in 0..device_count {
        // SAFETY: device_index is bounded by GetCount and devices remains alive.
        let Ok(device) = (unsafe { devices.Item(device_index) }) else {
            continue;
        };
        // SAFETY: device is a live COM interface; the activation parameters request the
        // documented audio session manager interface.
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            continue;
        };
        // SAFETY: manager is a live COM interface and owns the returned enumerator.
        let Ok(sessions) = (unsafe { manager.GetSessionEnumerator() }) else {
            continue;
        };
        // SAFETY: sessions is a live COM collection interface.
        let Ok(session_count) = (unsafe { sessions.GetCount() }) else {
            continue;
        };

        for session_index in 0..session_count {
            // SAFETY: session_index is bounded by GetCount and sessions remains alive.
            let Ok(session) = (unsafe { sessions.GetSession(session_index) }) else {
                continue;
            };
            // SAFETY: session is a live COM interface.
            let Ok(state) = (unsafe { session.GetState() }) else {
                continue;
            };
            if state != AudioSessionStateActive {
                continue;
            }
            let Ok(control) = session.cast::<IAudioSessionControl2>() else {
                continue;
            };
            // SAFETY: control is a live IAudioSessionControl2 interface.
            if unsafe { control.IsSystemSoundsSession() } == HRESULT(0) {
                continue;
            }
            // SAFETY: control is a live IAudioSessionControl2 interface.
            let Ok(process_id) = (unsafe { control.GetProcessId() }) else {
                continue;
            };
            if process_id != 0 {
                process_ids.insert(process_id);
            }
        }
    }

    Ok(process_ids)
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);

        // SAFETY: This initializes COM for the current thread with no reserved pointer.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.0 >= 0 {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(format!(
                "Failed to initialize COM for audio detection: {}.",
                format_hresult(result)
            ))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: A successful CoInitializeEx on this thread is balanced exactly once.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn format_hresult(result: HRESULT) -> String {
    format!("0x{:08X}", result.0 as u32)
}
