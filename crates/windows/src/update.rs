//! HTTPS transport for "Check for updates", on WinHTTP.
//!
//! WinHTTP ships with Windows, validates certificates against the system
//! store and follows the machine's proxy configuration, so no TLS or HTTP
//! crate is added. Only the controls process calls this; the worker never
//! performs network I/O. Redirects are refused, the response is capped, and
//! the body is handed to `any2odyssey_core::update` for parsing, which is
//! where every value from the server is checked.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use any2odyssey_core::update::{Request, MAX_RESPONSE_BYTES};
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

use crate::win::wide;

/// Resolve, connect, send and receive each get this long.
const TIMEOUT_MS: i32 = 10_000;
/// The whole body read must finish within this, whatever the per-call
/// timeouts allow: a server trickling bytes cannot hold the check thread.
const BODY_DEADLINE: Duration = Duration::from_secs(30);

/// A WinHTTP handle closed on drop, so every early return releases it.
struct Handle(*mut c_void);

impl Handle {
    fn open(raw: *mut c_void, call: &str) -> Result<Handle, String> {
        if raw.is_null() {
            Err(format!("{call}: {}", windows::core::Error::from_thread()))
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

/// Split `https://host/path` into host and path. Only the plain shape the
/// core module produces is accepted: no user information, port or query.
pub fn split_https_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find('/').unwrap_or(rest.len());
    let (host, path) = rest.split_at(end);
    if host.is_empty() || host.contains(['@', ':', '?', '#', '\\']) {
        return None;
    }
    Some((host, if path.is_empty() { "/" } else { path }))
}

fn set_u32(handle: &Handle, option: u32, value: u32) -> windows::core::Result<()> {
    unsafe {
        WinHttpSetOption(
            Some(handle.0.cast_const()),
            option,
            Some(&value.to_ne_bytes()),
        )
    }
}

fn status_code(request: &Handle) -> Result<u32, String> {
    let mut status = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
    }
    .map_err(|e| format!("status code: {e}"))?;
    Ok(status)
}

/// Perform `request` and return the response body as text. Any status other
/// than 200 is an error that names it: GitHub answers 403 when rate limited
/// and 404 when a repository has no published release.
pub fn fetch(request: &Request) -> Result<String, String> {
    let (host, path) =
        split_https_url(request.url).ok_or_else(|| format!("unsupported URL {}", request.url))?;
    let agent = wide(&request.user_agent);
    let session = Handle::open(
        unsafe {
            WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        },
        "WinHttpOpen",
    )?;
    unsafe { WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS) }
        .map_err(|e| format!("timeouts: {e}"))?;
    set_u32(
        &session,
        WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    )
    .map_err(|e| format!("redirect policy: {e}"))?;
    // Older Windows builds reject the TLS 1.3 flag as an invalid parameter.
    if set_u32(
        &session,
        WINHTTP_OPTION_SECURE_PROTOCOLS,
        WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3,
    )
    .is_err()
    {
        set_u32(
            &session,
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
        )
        .map_err(|e| format!("secure protocols: {e}"))?;
    }
    let host_w = wide(host);
    let connection = Handle::open(
        unsafe {
            WinHttpConnect(
                session.0,
                PCWSTR(host_w.as_ptr()),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            )
        },
        "WinHttpConnect",
    )?;
    let verb = wide("GET");
    let path_w = wide(path);
    let http = Handle::open(
        unsafe {
            WinHttpOpenRequest(
                connection.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        },
        "WinHttpOpenRequest",
    )?;
    // Belt and braces: the session-level policy is inherited, but the request
    // handle is where a redirect would actually be followed.
    set_u32(
        &http,
        WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    )
    .map_err(|e| format!("request redirect policy: {e}"))?;
    let headers: Vec<u16> = format!(
        "Accept: {}\r\nX-GitHub-Api-Version: {}\r\n",
        request.accept, request.api_version
    )
    .encode_utf16()
    .collect();
    unsafe { WinHttpAddRequestHeaders(http.0, &headers, WINHTTP_ADDREQ_FLAG_ADD) }
        .map_err(|e| format!("request headers: {e}"))?;
    unsafe { WinHttpSendRequest(http.0, None, None, 0, 0, 0) }
        .map_err(|e| format!("could not reach {host}: {e}"))?;
    unsafe { WinHttpReceiveResponse(http.0, std::ptr::null_mut()) }
        .map_err(|e| format!("no response from {host}: {e}"))?;
    let status = status_code(&http)?;
    if status != 200 {
        return Err(format!("{host} answered HTTP {status}"));
    }
    let mut body = Vec::new();
    let begun = Instant::now();
    loop {
        if begun.elapsed() > BODY_DEADLINE {
            return Err(format!(
                "response from {host} did not complete within {} seconds",
                BODY_DEADLINE.as_secs()
            ));
        }
        let mut available = 0u32;
        unsafe { WinHttpQueryDataAvailable(http.0, &mut available) }
            .map_err(|e| format!("reading response: {e}"))?;
        if available == 0 {
            break;
        }
        if body.len() + available as usize > MAX_RESPONSE_BYTES {
            return Err(format!("response larger than {MAX_RESPONSE_BYTES} bytes"));
        }
        let start = body.len();
        body.resize(start + available as usize, 0);
        let mut read = 0u32;
        unsafe {
            WinHttpReadData(
                http.0,
                body[start..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
        }
        .map_err(|e| format!("reading response: {e}"))?;
        body.truncate(start + read as usize);
        if read == 0 {
            break;
        }
    }
    String::from_utf8(body).map_err(|_| "response is not UTF-8 text".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use any2odyssey_core::update::LATEST_RELEASE_API;

    #[test]
    fn the_api_url_splits_into_host_and_path() {
        assert_eq!(
            split_https_url(LATEST_RELEASE_API),
            Some((
                "api.github.com",
                "/repos/jlobue10/Any2Odyssey/releases/latest"
            ))
        );
        assert_eq!(
            split_https_url("https://github.com"),
            Some(("github.com", "/"))
        );
        for bad in [
            "http://api.github.com/x",
            "https://user@api.github.com/x",
            "https://api.github.com:8443/x",
            "https://api.github.com?x",
            "https:///x",
            "api.github.com/x",
        ] {
            assert_eq!(split_https_url(bad), None, "{bad}");
        }
    }
}
