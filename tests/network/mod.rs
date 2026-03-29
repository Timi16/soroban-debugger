use std::io::ErrorKind;
use std::net::TcpListener;

/// Returns `true` when the test process can bind a loopback TCP socket.
///
/// In some environments (containers, CI sandboxes, restricted desktops)
/// loopback networking is blocked and `bind`/`connect` return `EPERM` or a
/// similar OS-level error.  Tests that require loopback networking must call
/// this function as an early guard and emit an explicit skip message before
/// returning when it returns `false`.
///
/// See `docs/remote-troubleshooting.md` — "Sandboxed / CI Environments" for
/// background and remediation steps.
pub fn can_bind_loopback() -> bool {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(_) => true,
        Err(e) => {
            let reason = if e.kind() == ErrorKind::PermissionDenied {
                "EPERM – loopback networking is not permitted in this environment \
                     (sandbox or container restriction). \
                     See docs/remote-troubleshooting.md for remediation steps."
                    .to_string()
            } else {
                format!("loopback networking restricted: {e}")
            };
            eprintln!("⚠️  Loopback bind check failed: {reason}");
            false
        }
    }
}
