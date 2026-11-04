use std::{ffi::c_void, io, ptr};

use semver::Version;
use serde::Deserialize;
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};

use crate::{config::UpdateChannel, win_util::wide_null};

const RELEASE_URL: &str = "https://github.com/TatshSiow/Winderust/releases/tag/";

#[derive(Debug, Clone)]
pub(crate) struct AvailableUpdate {
    pub url: String,
}

#[derive(Debug)]
pub(crate) struct ReleaseCheck {
    pub latest_version: String,
    pub available_update: Option<AvailableUpdate>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum UpdateCheckError {
    #[error("{operation} failed: {source}")]
    WinHttp {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("GitHub returned HTTP {0}")]
    HttpStatus(u32),
    #[error("GitHub release response exceeds 64 KiB")]
    ResponseTooLarge,
    #[error("Invalid GitHub release response: {0}")]
    InvalidResponse(#[from] serde_json::Error),
    #[error("GitHub returned no releases")]
    NoReleases,
    #[error("Invalid application version: {0}")]
    InvalidCurrentVersion(semver::Error),
}

impl UpdateCheckError {
    fn winhttp(operation: &'static str) -> Self {
        Self::WinHttp {
            operation,
            source: io::Error::last_os_error(),
        }
    }
}

pub(crate) fn check(channel: UpdateChannel) -> Result<ReleaseCheck, UpdateCheckError> {
    let body = get_latest_release(channel)?;
    release_check_from_response(&body, channel, env!("CARGO_PKG_VERSION"))
}

fn get_latest_release(channel: UpdateChannel) -> Result<Vec<u8>, UpdateCheckError> {
    let agent = wide_null(concat!("Winderust/", env!("CARGO_PKG_VERSION")));
    let host = wide_null("api.github.com");
    let path = wide_null(releases_path(channel));
    let get = wide_null("GET");

    // SAFETY: agent is terminated UTF-16; optional proxy strings are null as documented.
    let session = InternetHandle::new(
        unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        },
        "WinHttpOpen",
    )?;
    // SAFETY: session is a live WinHTTP session handle.
    if unsafe { WinHttpSetTimeouts(session.0, 5_000, 5_000, 5_000, 5_000) } == 0 {
        return Err(UpdateCheckError::winhttp("WinHttpSetTimeouts"));
    }

    // SAFETY: session is live and host is terminated UTF-16.
    let connection = InternetHandle::new(
        unsafe { WinHttpConnect(session.0, host.as_ptr(), 443, 0) },
        "WinHttpConnect",
    )?;
    // SAFETY: connection is live; method and path are terminated UTF-16; optional strings and
    // accepted types are null as documented.
    let request = InternetHandle::new(
        unsafe {
            WinHttpOpenRequest(
                connection.0,
                get.as_ptr(),
                path.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        },
        "WinHttpOpenRequest",
    )?;

    // SAFETY: request is live and no headers or request body are supplied.
    let sent = unsafe { WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) };
    if sent == 0 {
        return Err(UpdateCheckError::winhttp("WinHttpSendRequest"));
    }
    // SAFETY: request was sent successfully and no reserved response pointer is supplied.
    let received = unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) };
    if received == 0 {
        return Err(UpdateCheckError::winhttp("WinHttpReceiveResponse"));
    }

    let mut status = 0_u32;
    let mut status_bytes = std::mem::size_of_val(&status) as u32;
    // SAFETY: request has received a response; status is a writable DWORD and status_bytes
    // supplies its size in bytes. The standard numeric query requires no name or index.
    if unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&mut status as *mut u32).cast(),
            &mut status_bytes,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(UpdateCheckError::winhttp("WinHttpQueryHeaders"));
    }
    if status != 200 {
        return Err(UpdateCheckError::HttpStatus(status));
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
            return Err(UpdateCheckError::winhttp("WinHttpReadData"));
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read as usize]);
        if body.len() > 64 * 1024 {
            return Err(UpdateCheckError::ResponseTooLarge);
        }
    }

    Ok(body)
}
fn releases_path(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "/repos/TatshSiow/Winderust/releases/latest",
        UpdateChannel::PreRelease => "/repos/TatshSiow/Winderust/releases?per_page=1",
    }
}

#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
struct ReleaseTag {
    name: String,
    version: Version,
}

impl TryFrom<String> for ReleaseTag {
    type Error = semver::Error;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let version = Version::parse(name.strip_prefix('v').unwrap_or(&name))?;
        Ok(Self { name, version })
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: ReleaseTag,
}

fn release_check_from_response(
    body: &[u8],
    channel: UpdateChannel,
    current: &str,
) -> Result<ReleaseCheck, UpdateCheckError> {
    let release: Release = match channel {
        UpdateChannel::Stable => serde_json::from_slice(body)?,
        UpdateChannel::PreRelease => serde_json::from_slice::<Vec<Release>>(body)?
            .into_iter()
            .next()
            .ok_or(UpdateCheckError::NoReleases)?,
    };
    let tag = release.tag_name;
    let current = Version::parse(current).map_err(UpdateCheckError::InvalidCurrentVersion)?;
    let available_update = (tag.version > current).then(|| AvailableUpdate {
        url: format!("{RELEASE_URL}{}", tag.name),
    });
    Ok(ReleaseCheck {
        latest_version: tag.version.to_string(),
        available_update,
    })
}

struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(handle: *mut c_void, operation: &'static str) -> Result<Self, UpdateCheckError> {
        if handle.is_null() {
            Err(UpdateCheckError::winhttp(operation))
        } else {
            Ok(Self(handle))
        }
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
        let response = br#"[{"tag_name":"v0.2.0-alpha"}]"#;
        let newer = release_check_from_response(response, UpdateChannel::PreRelease, "0.1.1-alpha")
            .unwrap();
        assert_eq!(newer.latest_version, "0.2.0-alpha");
        assert_eq!(
            newer.available_update.unwrap().url,
            format!("{RELEASE_URL}v0.2.0-alpha")
        );
        let current =
            release_check_from_response(response, UpdateChannel::PreRelease, "0.2.0").unwrap();
        assert_eq!(current.latest_version, "0.2.0-alpha");
        assert!(current.available_update.is_none());
        assert_eq!(
            release_check_from_response(
                br#"{"tag_name":"v0.2.0"}"#,
                UpdateChannel::Stable,
                "0.1.0"
            )
            .unwrap()
            .latest_version,
            "0.2.0"
        );
        assert!(releases_path(UpdateChannel::Stable).ends_with("/latest"));
        assert!(releases_path(UpdateChannel::PreRelease).ends_with("per_page=1"));
    }

    #[test]
    fn release_responses_reject_invalid_tags_and_unexpected_shapes() {
        for body in [
            &b"invalid"[..],
            br#"{}"#,
            br#"{"tag_name":null}"#,
            br#"{"tag_name":42}"#,
            br#"{"tag_name":"vnext"}"#,
            br#"{"tag_name":""}"#,
            br#"{"tag_name":"v0.2.0","tag_name":"v0.3.0"}"#,
            br#"[{"tag_name":"v0.2.0"}]"#,
            b"\xff",
        ] {
            assert!(
                matches!(
                    release_check_from_response(body, UpdateChannel::Stable, "0.1.0"),
                    Err(UpdateCheckError::InvalidResponse(_))
                ),
                "accepted malformed release response: {body:?}"
            );
        }
        assert!(matches!(
            release_check_from_response(
                br#"{"tag_name":"v0.2.0"}"#,
                UpdateChannel::PreRelease,
                "0.1.0"
            ),
            Err(UpdateCheckError::InvalidResponse(_))
        ));
        assert!(matches!(
            release_check_from_response(b"[]", UpdateChannel::PreRelease, "0.1.0"),
            Err(UpdateCheckError::NoReleases)
        ));
        assert!(matches!(
            release_check_from_response(
                br#"{"tag_name":"0.2.0"}"#,
                UpdateChannel::Stable,
                "invalid"
            ),
            Err(UpdateCheckError::InvalidCurrentVersion(_))
        ));
    }

    #[test]
    fn release_metadata_allows_extra_fields_and_preserves_unprefixed_tags() {
        let check = release_check_from_response(
            br#"{"tag_name":"0.2.0","body":"release notes","assets":[]}"#,
            UpdateChannel::Stable,
            "0.1.0",
        )
        .unwrap();
        assert_eq!(check.latest_version, "0.2.0");
        assert_eq!(
            check.available_update.unwrap().url,
            format!("{RELEASE_URL}0.2.0")
        );
    }
}
