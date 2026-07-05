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

    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL)
            .map_err(|err| format!("Failed to create audio device enumerator: {err}."))?
    };
    let devices = unsafe {
        enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|err| format!("Failed to enumerate audio output devices: {err}."))?
    };
    let device_count = unsafe {
        devices
            .GetCount()
            .map_err(|err| format!("Failed to count audio output devices: {err}."))?
    };

    for device_index in 0..device_count {
        let Ok(device) = (unsafe { devices.Item(device_index) }) else {
            continue;
        };
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            continue;
        };
        let Ok(sessions) = (unsafe { manager.GetSessionEnumerator() }) else {
            continue;
        };
        let Ok(session_count) = (unsafe { sessions.GetCount() }) else {
            continue;
        };

        for session_index in 0..session_count {
            let Ok(session) = (unsafe { sessions.GetSession(session_index) }) else {
                continue;
            };
            let Ok(state) = (unsafe { session.GetState() }) else {
                continue;
            };
            if state != AudioSessionStateActive {
                continue;
            }
            let Ok(control) = session.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if unsafe { control.IsSystemSoundsSession() } == HRESULT(0) {
                continue;
            }
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
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn format_hresult(result: HRESULT) -> String {
    format!("0x{:08X}", result.0 as u32)
}
