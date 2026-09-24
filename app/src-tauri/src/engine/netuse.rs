//! Connects SMB shares (`\\server\share`) with a user name and password.
//!
//! Uses `WNetAddConnection2W` from the Win32 API directly rather than shelling
//! out to `net.exe use`: it is synchronous, never prompts on stdin (which
//! would hang a hidden console, e.g. with an empty password), and its return
//! code is the actual result of the connection attempt.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::NetworkManagement::WNet::{
    WNetAddConnection2W, WNetCancelConnection2W, WNetCloseEnum, WNetEnumResourceW,
    WNetOpenEnumW, NET_CONNECT_FLAGS, NETRESOURCEW, RESOURCETYPE_DISK, RESOURCE_CONNECTED,
    WNET_OPEN_ENUM_USAGE,
};

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `\\SERVER\share` -> `\\SERVER`. Windows allows only one set of credentials
/// per SMB server: if a session with a different user already exists (to any
/// share on that server), connecting fails with error 1219.
fn server_root(share: &str) -> Option<String> {
    let trimmed = share.trim_start_matches('\\');
    let server = trimmed.split('\\').next()?;
    if server.is_empty() {
        None
    } else {
        Some(format!(r"\\{server}"))
    }
}

/// A session with the server already exists using different credentials.
const ERROR_SESSION_CREDENTIAL_CONFLICT: u32 = 1219;

/// An existing connection to an SMB server.
struct ExistingConnection {
    remote: String,
    /// Drive letter (`Z:`) for a mapped network drive; empty for a
    /// connection without a drive letter (a folder opened in Explorer, a
    /// previous NASMirror run, ...).
    local: String,
}

/// Lists open connections to the same server as `share`.
/// Best effort: if enumeration fails, returns whatever was read so far.
fn server_connections(share: &str) -> Vec<ExistingConnection> {
    let Some(server) = server_root(share) else {
        return vec![];
    };
    let server_upper = server.to_uppercase();
    let mut found = vec![];
    unsafe {
        let mut henum = std::mem::zeroed();
        let opened = WNetOpenEnumW(
            RESOURCE_CONNECTED,
            RESOURCETYPE_DISK,
            WNET_OPEN_ENUM_USAGE(0),
            None,
            &mut henum,
        );
        if opened != WIN32_ERROR(0) {
            return found;
        }

        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let mut count: u32 = u32::MAX;
            let mut size = buffer.len() as u32;
            let rc = WNetEnumResourceW(
                henum,
                &mut count,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                &mut size,
            );
            // End of list (ERROR_NO_MORE_ITEMS) or any error: stop.
            if rc != WIN32_ERROR(0) || count == 0 {
                break;
            }
            let items =
                std::slice::from_raw_parts(buffer.as_ptr() as *const NETRESOURCEW, count as usize);
            for item in items {
                if item.lpRemoteName.is_null() {
                    continue;
                }
                let Ok(remote) = item.lpRemoteName.to_string() else {
                    continue;
                };
                let remote_upper = remote.to_uppercase();
                if remote_upper == server_upper
                    || remote_upper.starts_with(&format!("{server_upper}\\"))
                {
                    let local = if item.lpLocalName.is_null() {
                        String::new()
                    } else {
                        item.lpLocalName.to_string().unwrap_or_default()
                    };
                    found.push(ExistingConnection { remote, local });
                }
            }
        }
        let _ = WNetCloseEnum(henum);
    }
    found
}

#[derive(Debug, thiserror::Error)]
pub enum NetUseError {
    #[error("could not connect to {share}: {}", describe_code(.code))]
    Connect { share: String, code: u32 },
    #[error(
        "{server} is already connected as a different user through {drives}. \
         Disconnect those network drives, or use the same user in this job"
    )]
    MappedDriveConflict { server: String, drives: String },
}

/// Turns the most common network error codes into an actionable message.
fn describe_code(code: &u32) -> String {
    let hint = match *code {
        53 => "network path not found (is the server name right, and the machine on?)",
        67 => "that shared folder does not exist on the server",
        86 | 1326 => "wrong user name or password",
        1219 => "Windows already has a session with that server under a different user",
        1244 | 5 => "access denied for that user",
        1231 | 1232 => "the server is not reachable on the network",
        _ => return format!("network error {code}"),
    };
    format!("{hint} (code {code})")
}

/// Drops any existing connection to `share`. The result is deliberately
/// ignored: having nothing connected is not an error.
pub fn disconnect(share: &str) {
    let wide = to_wide(share);
    unsafe {
        let _ = WNetCancelConnection2W(PCWSTR(wide.as_ptr()), NET_CONNECT_FLAGS(0), true);
    }
}

fn add_connection(share: &str, user: &str, password: &str) -> u32 {
    let share_w = to_wide(share);
    let user_w = to_wide(user);
    let pass_w = to_wide(password);
    let nr = NETRESOURCEW {
        dwType: RESOURCETYPE_DISK,
        lpLocalName: PWSTR::null(),
        lpRemoteName: PWSTR(share_w.as_ptr() as *mut u16),
        lpProvider: PWSTR::null(),
        ..Default::default()
    };
    let result: WIN32_ERROR = unsafe {
        WNetAddConnection2W(
            &nr,
            PCWSTR(pass_w.as_ptr()),
            PCWSTR(user_w.as_ptr()),
            NET_CONNECT_FLAGS(0),
        )
    };
    result.0
}

/// Connects `share` with the given credentials. Blocking: call from
/// `spawn_blocking`. Never prompts; on failure it returns the Win32 error code
/// so it can be explained (bad credentials, missing share, server offline, ...).
///
/// If Windows already has a session with that server under another user
/// (error 1219), only connections WITHOUT a drive letter (transient ones:
/// Explorer, previous runs) are closed and the connection is retried. The
/// user's mapped network drives are never disconnected: if they cause the
/// conflict, the returned error names them.
pub fn connect(share: &str, user: &str, password: &str) -> Result<(), NetUseError> {
    disconnect(share);
    let mut code = add_connection(share, user, password);

    if code == ERROR_SESSION_CREDENTIAL_CONFLICT {
        let existing = server_connections(share);
        for conn in existing.iter().filter(|c| c.local.is_empty()) {
            disconnect(&conn.remote);
        }
        code = add_connection(share, user, password);

        let mapped: Vec<String> = existing
            .iter()
            .filter(|c| !c.local.is_empty())
            .map(|c| format!("{} ({})", c.local, c.remote))
            .collect();
        if code == ERROR_SESSION_CREDENTIAL_CONFLICT && !mapped.is_empty() {
            return Err(NetUseError::MappedDriveConflict {
                server: server_root(share).unwrap_or_else(|| share.to_string()),
                drives: mapped.join(", "),
            });
        }
    }

    if code != 0 {
        return Err(NetUseError::Connect {
            share: share.to_string(),
            code,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_root_of_share() {
        assert_eq!(server_root(r"\\NAS\backup").as_deref(), Some(r"\\NAS"));
        assert_eq!(server_root(r"\\nas").as_deref(), Some(r"\\nas"));
        assert_eq!(server_root(r"\\"), None);
    }

    #[test]
    fn describe_known_and_unknown_codes() {
        assert!(describe_code(&1326).contains("wrong user name or password"));
        assert!(describe_code(&67).contains("shared folder"));
        assert_eq!(describe_code(&999), "network error 999");
    }
}
