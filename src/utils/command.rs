//! Utilities for process execution.

use std::ffi::OsStr;
use std::process::{Command, Output, Stdio};

use anyhow::{Result, bail};

use super::strings;

/// Shared setup for [`Command`].
///
/// Prevents inheriting environment variables ([`Command::env_clear`]), uses `/` as current directory, and
/// set up piped [`stdout`] and [`stderr`], but no [`stdin`].
///
/// [`stdin`]: Command::stdin
/// [`stdout`]: Command::stdout
/// [`stderr`]: Command::stderr
pub fn command(program: impl AsRef<OsStr>) -> Command {
    log::trace!("command: program={:?}", program.as_ref());
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Verify command output.
///
/// Check exit status, stderr and (optionally) stdout.
pub fn check(name: &str, output: Output, show_stdout: bool) -> Result<Vec<u8>> {
    log::trace!(
        "{name}: exit={}, #lines stdout={}, #lines stderr={}",
        output.status,
        strings::lines(&output.stdout).count(),
        strings::lines(&output.stderr).count()
    );
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        match (output.status.code(), stderr.trim().is_empty()) {
            (Some(code), false) => bail!("{name} failed (status = {code}): {stderr}"),
            (Some(code), true) => bail!("{name} failed (status = {code})"),
            (None, false) => bail!("{name} failed: {stderr}"),
            (None, true) => bail!("{name} failed"),
        }
    }
    for line in strings::lines(&output.stderr) {
        log::warn!("{name}: {}", line.escape_ascii());
    }
    if show_stdout {
        for line in strings::lines(&output.stdout) {
            log::info!("{name}: {}", line.escape_ascii());
        }
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::*;

    macro_rules! output {
        ($status:literal, $stdout:expr, $stderr:expr) => {
            Output {
                status: ExitStatus::from_raw($status),
                stdout: $stdout.into(),
                stderr: $stderr.into(),
            }
        };
    }

    #[test]
    fn check_output() {
        let out = check("first", output!(0, b"out1", b"err1"), true).unwrap();
        assert_eq!(out.escape_ascii().to_string(), "out1");

        let err = check("second", output!(0x0A80, b"out2", b"err2"), false).unwrap_err();
        assert_eq!(err.to_string(), "second failed (status = 10): err2");

        let err = check("third", output!(0x0001, b"out3", b"err3"), true).unwrap_err();
        assert_eq!(err.to_string(), "third failed: err3");

        let err = check("fourth", output!(0x0180, b"empty stderr", b"    "), false).unwrap_err();
        assert_eq!(err.to_string(), "fourth failed (status = 1)");

        let err = check("fifth", output!(0x0018, b"", b"   "), true).unwrap_err();
        assert_eq!(err.to_string(), "fifth failed");
    }
}
