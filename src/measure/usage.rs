//! Access and display resource usage.

use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::{Result, bail};
use byte_unit::{Byte, Unit};
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;

/// Convert from `libc`'s [`timeval`](libc::timeval) to `chrono`'s [`Duration`].
#[must_use]
const fn duration(timeval: libc::timeval) -> Duration {
    #![expect(clippy::cast_sign_loss, reason = "values are checked before cast")]

    assert!(timeval.tv_sec >= 0, "negative duration is considered an error in Rust");
    let secs = Duration::from_secs(timeval.tv_sec as u64);

    assert!(timeval.tv_usec >= 0, "negative duration is considered an error in Rust");
    let usecs = Duration::from_micros(timeval.tv_usec as u64);

    secs.checked_add(usecs)
        .expect("conversion from timeval to Duration overflowed")
}

/// Convert size from kilobytes (`i64`) to [`Byte`].
#[must_use]
const fn kibibytes(size: i64) -> Byte {
    Byte::from_i64_with_unit(size, Unit::KiB).expect("negative kibibyte size found")
}

/// Convert to non-negative value.
#[must_use]
const fn count(value: i64) -> u64 {
    #![expect(clippy::cast_sign_loss, reason = "values are checked before cast")]
    assert!(value >= 0, "negative natural value found");
    value as u64
}

/// Resource usage statistics for a finished process.
///
/// See [getrusage(2)](https://man.archlinux.org/man/getrusage.2.en) and
/// [Resource Usage](https://www.gnu.org/software/libc/manual/html_node/Resource-Usage.html).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Resource usage stats.
    usage: libc::rusage,
    /// Child wait status.
    wait_status: WaitStatus,
    /// Child exit status.
    exit_status: ExitStatus,
    /// Real (wall) time.
    wall_time: Duration,
    /// Virtual (CPU) time.
    monotonic_time: Duration,
}

impl Stats {
    /// Check exit status and build valid resource usage data.
    pub(super) fn from_result(
        pid: Pid,
        result: i32,
        wstatus: i32,
        usage: libc::rusage,
        wall_time: Duration,
        monotonic_time: Duration,
    ) -> Result<Self> {
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
            wall_time,
            monotonic_time,
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

    /// User CPU time used.
    ///
    /// Time spent executing user instructions.
    ///
    /// # Panics
    ///
    /// If [`Duration`] overflows or a negative value was received.
    #[inline]
    #[must_use]
    pub const fn user_time(&self) -> Duration {
        duration(self.usage.ru_utime)
    }

    /// System CPU time used.
    ///
    /// Time spent in operating system code on behalf of processes.
    ///
    /// # Panics
    ///
    /// If [`Duration`] overflows or a negative value was received.
    #[inline]
    #[must_use]
    pub const fn system_time(&self) -> Duration {
        duration(self.usage.ru_stime)
    }

    /// Elapsed virtual (CPU) time.
    ///
    /// Measured with [`Instant`](std::time::Instant). Should be equivalent to [`user_time`](Self::user_time) plus
    /// [`system_time`](Self::system_time).
    #[inline]
    #[must_use]
    pub const fn virtual_time(&self) -> Duration {
        self.monotonic_time
    }

    /// Elapsed real (wall) time.
    ///
    /// Measured with [`SystemTime`](std::time::SystemTime).
    #[inline]
    #[must_use]
    pub const fn real_time(&self) -> Duration {
        self.wall_time
    }

    /// Maximum resident set size.
    ///
    /// The maximum resident set size used, in kilobytes. That is, the maximum number of kilobytes of physical memory
    /// that processes used simultaneously.
    ///
    /// Since Linux 2.6.32.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn max_rss(&self) -> Byte {
        kibibytes(self.usage.ru_maxrss)
    }

    /// Integral shared memory size. (unmaintained)
    ///
    /// An integral value expressed in kilobytes times ticks of execution, which indicates the amount of memory used by
    /// text that was shared with other processes.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn integral_shared_rss(&self) -> Byte {
        kibibytes(self.usage.ru_ixrss)
    }

    /// Integral unshared data size. (unmaintained)
    ///
    /// An integral value expressed in kilobytes times ticks of execution, which is the amount of unshared memory used
    /// for data.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn integral_data_rss(&self) -> Byte {
        kibibytes(self.usage.ru_idrss)
    }

    /// Integral unshared stack size. (unmaintained)
    ///
    /// An integral value expressed in kilobytes times ticks of execution, which is the amount of unshared memory used
    /// for stack space.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn integral_stack_rss(&self) -> Byte {
        kibibytes(self.usage.ru_isrss)
    }

    /// Page reclaims (soft page faults).
    ///
    /// The number of page faults which were serviced without requiring any I/O.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn minor_page_faults(&self) -> u64 {
        count(self.usage.ru_minflt)
    }

    /// Page reclaims (hard page faults).
    ///
    /// The number of page faults which were serviced by doing I/O.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn major_page_faults(&self) -> u64 {
        count(self.usage.ru_majflt)
    }

    /// Number of swaps. (unmaintained)
    ///
    /// The number of times processes was swapped entirely out of main memory.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn num_swaps(&self) -> u64 {
        count(self.usage.ru_nswap)
    }

    /// Block input operations.
    ///
    /// The number of times the file system had to read from the disk on behalf of processes.
    ///
    /// Since Linux 2.6.22.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn input_blocked(&self) -> u64 {
        count(self.usage.ru_inblock)
    }

    /// Block output operations.
    ///
    /// The number of times the file system had to write to the disk on behalf of processes.
    ///
    /// Since Linux 2.6.22.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn output_blocked(&self) -> u64 {
        count(self.usage.ru_oublock)
    }

    /// IPC messages sent. (unmaintained)
    ///
    /// Number of IPC messages sent.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn ipc_msg_snd(&self) -> u64 {
        count(self.usage.ru_msgsnd)
    }

    /// IPC messages received. (unmaintained)
    ///
    /// Number of IPC messages received.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn ipc_msg_rcv(&self) -> u64 {
        count(self.usage.ru_msgrcv)
    }

    /// Signals received. (unmaintained)
    ///
    /// Number of signals received.
    ///
    /// This field is currently unused on Linux.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn num_signals(&self) -> u64 {
        count(self.usage.ru_nsignals)
    }

    /// Voluntary context switches.
    ///
    /// The number of times processes voluntarily invoked a context switch (usually to wait for some service).
    ///
    /// Since Linux 2.6.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn num_vol_ctx_sw(&self) -> u64 {
        count(self.usage.ru_nvcsw)
    }

    /// Involuntary context switches.
    ///
    /// The number of times an involuntary context switch took place (because a time slice expired, or another process
    /// of higher priority was scheduled).
    ///
    /// Since Linux 2.6.
    ///
    /// # Panics
    ///
    /// Negative value from `wait4`.
    #[inline]
    #[must_use]
    pub const fn num_inv_ctx_sw(&self) -> u64 {
        count(self.usage.ru_nivcsw)
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::{Instant, SystemTime};

    use nix::errno::Errno;

    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::*;

    fn mock_usage() -> Stats {
        let (wall_time, monotonic_time) = (SystemTime::now(), Instant::now());
        sleep(Duration::from_millis(1));

        // SAFETY: libc structs can be zeroed
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: valid pointer
        let res = unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) };
        Errno::result(res).unwrap();

        let pid = Pid::this();
        let real_time = wall_time.elapsed().unwrap();
        let virtual_time = monotonic_time.elapsed();

        Stats::from_result(pid, pid.as_raw(), 0x0080, usage, real_time, virtual_time).unwrap()
    }

    #[test]
    #[expect(clippy::integer_division_remainder_used, reason = "used for tests only")]
    fn no_panic() {
        let usage = mock_usage();

        assert_eq!(usage.exit_code(), 0);
        assert_eq!(usage.pid(), Pid::this());

        assert_eq!(usage.user_time().as_secs(), 0);
        assert_eq!(usage.system_time().as_secs(), 0);
        assert_eq!(usage.virtual_time().as_secs(), 0);
        assert_eq!(usage.real_time().as_secs(), 0);
        assert_ne!(usage.real_time().as_nanos(), 0);

        assert_eq!(format!("{:.1}", usage.max_rss().get_adjusted_unit(Unit::GiB)), "0.0 GiB");
        assert_ne!(format!("{:.0}", usage.max_rss().get_adjusted_unit(Unit::MiB)), "0 MiB");
        assert_ne!(usage.minor_page_faults(), 0);
        assert_eq!(usage.minor_page_faults() / 10_000, 0);
        assert_eq!(usage.major_page_faults() / 1_000, 0);
        assert_eq!(usage.input_blocked() / 1_000, 0);
        assert_eq!(usage.output_blocked() / 1_000, 0);
        assert_ne!(usage.num_vol_ctx_sw(), 0);
        assert_eq!(usage.num_vol_ctx_sw() / 10_000, 0);
        assert_ne!(usage.num_inv_ctx_sw(), 0);
        assert_eq!(usage.num_inv_ctx_sw() / 1_000, 0);

        // not implemented in Linux
        assert_eq!(usage.integral_shared_rss().as_u128(), 0);
        assert_eq!(usage.integral_data_rss().as_u128(), 0);
        assert_eq!(usage.integral_stack_rss().as_u128(), 0);
        assert_eq!(usage.num_swaps(), 0);
        assert_eq!(usage.ipc_msg_snd(), 0);
        assert_eq!(usage.ipc_msg_rcv(), 0);
        assert_eq!(usage.num_signals(), 0);
    }
}
