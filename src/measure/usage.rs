//! Access and display resource usage.

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;

use anyhow::{Result, bail};
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;

/// Resource usage statistics for a finished process.
///
/// See [getrusage(2)](https://man.archlinux.org/man/getrusage.2.en).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Resource usage stats.
    usage: libc::rusage,
    /// Child wait status.
    wait_status: WaitStatus,
    /// Child exit status.
    exit_status: ExitStatus,
}

impl Usage {
    /// Check exit status and build valid resource usage data.
    pub(super) fn from_result(pid: Pid, result: i32, wstatus: i32, usage: libc::rusage) -> Result<Self> {
        let wait_status = WaitStatus::from_raw(Pid::from_raw(result), wstatus)?;
        match wait_status.pid() {
            Some(output_pid) if output_pid == pid => (),
            Some(output_pid) => bail!("different PID: expected={pid}, received={output_pid}"),
            None => bail!("no process measured"),
        }

        let exit_status = ExitStatus::from_raw(wstatus);
        log::trace!("usage: pid={pid}, wait_status={wait_status:?}, exit_status={exit_status:?}");
        if exit_status.code().is_none() {
            bail!("process did not exit, discarding usage");
        }

        Ok(Self {
            usage,
            wait_status,
            exit_status,
        })
    }

    #[inline]
    #[must_use]
    pub const fn wait_status(&self) -> WaitStatus {
        self.wait_status
    }

    #[inline]
    #[must_use]
    pub fn pid(&self) -> Pid {
        self.wait_status()
            .pid()
            .unwrap_or_else(|| unreachable!("PID was reasolved from wait status before"))
    }

    #[inline]
    #[must_use]
    pub const fn exit_status(&self) -> ExitStatus {
        self.exit_status
    }

    #[inline]
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.exit_status()
            .code()
            .unwrap_or_else(|| unreachable!("exit code was reasolved from exit status before"))
    }
}
