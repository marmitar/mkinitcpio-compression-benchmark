//! Run command and measure resource usage.

use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::process::{Child, Output};

use anyhow::{Context, Result, bail};
use nix::errno::Errno;
use nix::unistd::Pid;

mod usage;

pub use usage::Usage;

use crate::utils::command;

/// Execute command and measure resource usage.
///
/// Standard output and standard error are logged.
///
/// # Errors
///
/// Fails if the program exits with non-zero status, or any other runtime issue.
pub fn exec(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<Usage> {
    let mut args = args.into_iter();
    let Some(program) = args.next() else {
        bail!("missing binary to be executed");
    };

    let process = command::command(&program).args(args).spawn()?;
    let (output, usage) = wait_exit(process)?;

    let name = String::from_utf8_lossy(program.as_ref().as_bytes());
    command::check(&name, output, true)?;
    Ok(usage)
}

/// Wait for process to exit, capturing its output and resource usage.
fn wait_exit(process: Child) -> Result<(Output, Usage)> {
    let pid = process
        .id()
        .try_into()
        .map(Pid::from_raw)
        .with_context(|| format!("invalid PID: {}", process.id()))?;

    let (stdout, stderr) = capture_output(process)?;
    let usage = wait4(pid)?;

    let output = Output {
        status: usage.exit_status(),
        stdout,
        stderr,
    };
    Ok((output, usage))
}

/// Capture stdout and stderr from child.
fn capture_output(mut process: Child) -> Result<(Vec<u8>, Vec<u8>)> {
    std::mem::drop(process.stdin.take());
    log::trace!("capture_output: stdin");

    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    if let Some(mut out) = process.stdout.take() {
        out.read_to_end(&mut stdout)?;
    }
    log::trace!("capture_output: stdout");

    if let Some(mut err) = process.stderr.take() {
        err.read_to_end(&mut stderr)?;
    }
    log::trace!("capture_output: stderr");

    Ok((stdout, stderr))
}

/// Wait for process to exit and return its resource usage.
///
/// For more details, see [wait4(2)](https://man.archlinux.org/man/wait4.2).
fn wait4(pid: Pid) -> Result<Usage> {
    log::debug!("wait4: pid={pid}, options not supported in modern Linux");

    let mut wstatus: i32 = 0;
    // SAFETY: libc structs have valid all-zero byte-patterns
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: all pointers are valid for FFI with wait4 here
    let result = unsafe { libc::wait4(pid.as_raw(), &raw mut wstatus, 0, &raw mut usage) };
    // NOTE: don't call anything before calling result, so it won't overwrite errno
    let errno = Errno::last();

    log::trace!("wait4: result={result}, errno={errno}, wstatus={wstatus}, usage={usage:?}");
    if result == -1 {
        return Err(errno.into());
    }
    Usage::from_result(pid, result, wstatus, usage)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::*;

    #[test]
    fn exec_works() {
        let usage = exec(["true"]).unwrap();
        assert_ne!(usage.pid(), Pid::from_raw(0));
        assert_ne!(usage.pid(), Pid::from_raw(-1));
        assert_eq!(usage.exit_code(), 0);

        let error = exec(["false"]).unwrap_err();
        assert_eq!(error.to_string(), "false failed (status = 1)");

        let error = exec(["emtpy"; 0]).unwrap_err();
        assert_eq!(error.to_string(), "missing binary to be executed");

        let usage = exec(["echo", "hi"]).unwrap();
        assert_ne!(usage.pid(), Pid::from_raw(0));
        assert_ne!(usage.pid(), Pid::from_raw(-1));
        assert_eq!(usage.exit_code(), 0);
    }
}
