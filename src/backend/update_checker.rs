use std::{ffi::c_void, ptr};

use semver::Version;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
};

use crate::{config::UpdateChannel, win_util::wide_null};

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
    let agent = wide_null(concat!("Winderust/", env!("CARGO_PKG_VERSION")));
    let host = wide_null("api.github.com");
    let path = wide_null(releases_path(channel));
    let get = wide_null("GET");

    // SAFETY: agent is terminated UTF-16; optional proxy strings are null as documented.
    let session = InternetHandle::new(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    })?;
    // SAFETY: session is a live WinHTTP session handle.
    unsafe { WinHttpSetTimeouts(session.0, 5_000, 5_000, 5_000, 5_000) };

    // SAFETY: session is live and host is terminated UTF-16.
    let connection =
        InternetHandle::new(unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) })?;
    // SAFETY: connection is live; method and path are terminated UTF-16; optional strings and
    // accepted types are null as documented.
    let request = InternetHandle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            get.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    })?;

    // SAFETY: request is live and no headers or request body are supplied.
    let sent = unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) };
    if sent == 0 {
        return None;
    }
    // SAFETY: request was sent successfully and no reserved response pointer is supplied.
    let received = unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) };
    if received == 0 {
        return None;
    }

    let mut body = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let mut read = 0;
        // SAFETY: request is live; buffer and read remain writable for the supplied sizes.
        let read_ok = unsafe {
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast::<c_void>(),
                buffer.len() as u32,
                &mut read,
            )
        };
        if read_ok == 0 {
            return None;
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read as usize]);
        if body.len() > 64 * 1024 {
            return None;
        }
    }

    String::from_utf8(body).ok()
}
fn releases_path(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "/repos/TatshSiow/Winderust/releases/latest",
        UpdateChannel::PreRelease => "/repos/TatshSiow/Winderust/releases?per_page=1",
    }
}

fn release_check_from_response(body: &str, current: &str) -> Option<ReleaseCheck> {
    let response: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = response
        .get("tag_name")
        .or_else(|| response.get(0)?.get("tag_name"))?
        .as_str()?;
    let latest = Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    let available_update = (latest > Version::parse(current).ok()?).then(|| AvailableUpdate {
        url: format!("{RELEASE_URL}{tag}"),
    });
    Some(ReleaseCheck {
        latest_version: latest.to_string(),
        available_update,
    })
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
        assert_eq!(
            release_check_from_response(r#"{"tag_name":"v0.2.0"}"#, "0.1.0")
                .unwrap()
                .latest_version,
            "0.2.0"
        );
        assert!(releases_path(UpdateChannel::Stable).ends_with("/latest"));
        assert!(releases_path(UpdateChannel::PreRelease).ends_with("per_page=1"));
    }
}
