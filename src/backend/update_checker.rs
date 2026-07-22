use std::{ffi::c_void, ptr};

use semver::Version;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
};

use crate::config::UpdateChannel;

const RELEASE_URL: &str = "https://github.com/TatshSiow/Winderust/releases/tag/";

#[derive(Clone)]
pub(crate) struct AvailableUpdate {
    pub url: String,
}

pub(crate) struct ReleaseCheck {
    pub latest_version: String,
    pub available_update: Option<AvailableUpdate>,
}

pub(crate) fn check(channel: UpdateChannel) -> Result<ReleaseCheck, ()> {
    let body = get_latest_release(channel).ok_or(())?;
    release_check_from_response(&body, env!("CARGO_PKG_VERSION")).ok_or(())
}

fn get_latest_release(channel: UpdateChannel) -> Option<String> {
    let agent = wide(concat!("Winderust/", env!("CARGO_PKG_VERSION")));
    let host = wide("api.github.com");
    let path = wide(releases_path(channel));
    let get = wide("GET");

    // SAFETY: All WinHTTP strings are terminated UTF-16, optional buffers are null as documented,
    // returned handles are checked and owned by InternetHandle, and read buffers stay live for
    // each call.
    unsafe {
        let session = InternetHandle::new(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5_000, 5_000, 5_000, 5_000);

        let connection = InternetHandle::new(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
        let request = InternetHandle::new(WinHttpOpenRequest(
            connection.0,
            get.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;

        if WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0
        {
            return None;
        }

        let mut body = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let mut read = 0;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast::<c_void>(),
                buffer.len() as u32,
                &mut read,
            ) == 0
                || read == 0
            {
                break;
            }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > 64 * 1024 {
                return None;
            }
        }

        String::from_utf8(body).ok()
    }
}

fn releases_path(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "/repos/TatshSiow/Winderust/releases/latest",
        UpdateChannel::PreRelease => "/repos/TatshSiow/Winderust/releases?per_page=1",
    }
}

fn release_check_from_response(body: &str, current: &str) -> Option<ReleaseCheck> {
    let tag = body.split_once("\"tag_name\":")?.1.trim_start();
    let tag = tag.strip_prefix('"')?.split('"').next()?;
    let latest = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    let available_update = (latest > Version::parse(current).ok()?).then(|| AvailableUpdate {
        url: format!("{RELEASE_URL}{tag}"),
    });
    Some(ReleaseCheck {
        latest_version: latest.to_string(),
        available_update,
    })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(handle: *mut c_void) -> Option<Self> {
        (!handle.is_null()).then_some(Self(handle))
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        // SAFETY: self.0 is a non-null WinHTTP handle owned by this wrapper and closed once.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_response_reports_latest_and_only_offers_newer_versions() {
        let response = r#"[{"tag_name":"v0.2.0-alpha"}]"#;
        let newer = release_check_from_response(response, "0.1.1-alpha").unwrap();
        assert_eq!(newer.latest_version, "0.2.0-alpha");
        assert!(newer.available_update.is_some());
        let current = release_check_from_response(response, "0.2.0").unwrap();
        assert_eq!(current.latest_version, "0.2.0-alpha");
        assert!(current.available_update.is_none());
        assert!(release_check_from_response("invalid", "0.1.0").is_none());
        assert!(releases_path(UpdateChannel::Stable).ends_with("/latest"));
        assert!(releases_path(UpdateChannel::PreRelease).ends_with("per_page=1"));
    }
}
