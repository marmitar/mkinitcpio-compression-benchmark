//! Invoking Bash.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use format_bytes::{format_bytes, write_bytes};

use crate::utils::strings::lines;

/// Run a restricted Bash shell.
///
/// # Errors
///
/// Runtime or bash errors.
pub fn rbash(commands: impl AsRef<[u8]>) -> Result<Vec<u8>> {
    rbash_at(commands.as_ref(), Path::new("/"))
}

/// Run a restricted Bash shell at `dir`.
///
/// # Errors
///
/// Runtime or bash errors.
pub fn rbash_at(commands: &[u8], dir: &Path) -> Result<Vec<u8>> {
    log::trace!("rbash: dir={}", dir.display());
    let mut child = Command::new("/usr/bin/bash")
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("-r")
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        bail!("no stdin pipe provided to communicate with bash");
    };

    log::trace!("rbash: commands={}", commands.escape_ascii());
    write_bytes!(&mut stdin, b"set -o errexit\n")?;
    write_bytes!(&mut stdin, b"{}\n", commands)?;
    write_bytes!(&mut stdin, b"exit\n")?;
    std::mem::drop(stdin);

    let output = child.wait_with_output()?;
    log::trace!(
        "rbash: exit={}, #lines stdout={}, #lines stderr={}",
        output.status,
        lines(&output.stdout).count(),
        lines(&output.stderr).count()
    );

    if !output.status.success() {
        let message = "bash script failed";
        let stderr = String::from_utf8_lossy(&output.stderr);

        match (output.status.code(), stderr.trim().is_empty()) {
            (Some(code), false) => bail!("{message} (status = {code}): {stderr}"),
            (Some(code), true) => bail!("{message} (status = {code})"),
            (None, false) => bail!("{message}: {stderr}"),
            (None, true) => bail!("{message}"),
        }
    }

    for line in lines(&output.stderr) {
        log::error!("rbash: {}", line.escape_ascii());
    }
    Ok(output.stdout)
}

/// Run a restricted Bash shell and show value of `OUTPUT` variable.
///
/// # Errors
///
/// Runtime or bash errors.
pub fn rbash_with_output(commands: impl AsRef<[u8]>) -> Result<String> {
    rbash_with_output_at(commands.as_ref(), Path::new("/"))
}

/// Run a restricted Bash shell at `dir` and show value of `OUTPUT` variable.
///
/// # Errors
///
/// Runtime or bash errors.
pub fn rbash_with_output_at(commands: &[u8], dir: &Path) -> Result<String> {
    let commands_with_output = format_bytes!(
        b"{}
        declare | grep -E '^OUTPUT=' || true",
        commands
    );

    let environment = String::from_utf8(rbash_at(&commands_with_output, dir)?)?;
    let mut values = environment.lines().filter_map(|line| line.strip_prefix("OUTPUT="));

    let Some(result) = values.next() else {
        bail!("missing OUTPUT variable");
    };
    if values.next().is_some() {
        bail!("multiple OUTPUT variables");
    }

    Ok(result.into())
}

/// Resolve file then split into directory and filename.
///
/// # Examples
///
/// ```ignore https://github.com/rust-lang/rust/issues/67295
/// # use std::path::Path;
/// # use std::ffi::OsStr;
/// # use ::mkinitcpio_compression_benchmark::bash::exec::resolve_file;
/// std::env::set_current_dir("/usr")?;
/// let (dir, file) = resolve_file("bin/bash".as_ref())?;
/// assert_eq!(dir, Path::new("/usr/bin"));
/// assert_eq!(file, OsStr::new("bash"));
/// # anyhow::Ok(())
/// ```
///
/// # Errors
///
/// Path could not be found or it is not a file.
pub fn resolve_file(input_path: &Path) -> Result<(PathBuf, OsString)> {
    let path = input_path.canonicalize()?;
    log::trace!("resolve_file: {} => {}", input_path.display(), path.display());

    if !path.is_file() {
        bail!("not a file: {} (resolved from {})", path.display(), input_path.display());
    }
    let (Some(dir), Some(file)) = (path.parent(), path.file_name()) else {
        bail!("invalid path: {} (resolved from {})", path.display(), input_path.display());
    };
    Ok((dir.to_owned(), file.to_owned()))
}

#[cfg(test)]
mod rbash {
    use pretty_assertions::{assert_eq, assert_matches};
    use test_log::test;

    use super::*;

    #[test]
    fn captures_stdout() {
        let output = rbash(b"echo 'test string'").unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "test string\n", "keeps raw newline");

        let output = rbash(b"echo -n 'test string'").unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "test string", "don't add newlines");
    }

    #[test]
    fn check_for_command_failures() {
        let err = rbash(b"echo()").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("syntax error"), "syntax error captured");

        let err = rbash(b"exit 55").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("(status = 55)"), "exit code reported");

        let err = rbash(b"printf '%z'").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("missing format character"), "bash printf error");

        let err = rbash_at(b"whoami --wrong=arg", Path::new("/usr/bin")).unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("unrecognized option"), "external binary error");
    }

    #[test]
    fn captures_output_variable() {
        let output = rbash_with_output(b"OUTPUT=text").unwrap();
        assert_eq!(output, "text");

        let output = rbash_with_output(b"OUTPUT=requires\\ quotes").unwrap();
        assert_eq!(output, "'requires quotes'");

        let output = rbash_with_output(b"OUTPUT='needs\nescaping'").unwrap();
        assert_eq!(output, "$'needs\\nescaping'");

        let output = rbash_with_output(b"OUTPUT=('works with' arrays)").unwrap();
        assert_eq!(output, "([0]=\"works with\" [1]=\"arrays\")");
    }

    #[test]
    fn expects_single_output_variable() {
        let err = rbash_with_output(b"echo something").unwrap_err();
        assert_eq!(err.to_string(), "missing OUTPUT variable");

        let command = b"
            OUTPUT='first var'
            echo OUTPUT='second var'
        ";
        let err = rbash_with_output(command).unwrap_err();
        assert_eq!(err.to_string(), "multiple OUTPUT variables");
    }
}

#[cfg(test)]
mod resolve_file {
    use std::ffi::OsStr;

    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::*;

    #[test]
    fn at_absolute_path() {
        let (dir, file) = resolve_file("/usr/bin/echo".as_ref()).unwrap();

        assert_eq!(dir, Path::new("/usr/bin"));
        assert_eq!(file, OsStr::new("echo"));
    }

    #[test]
    fn at_relative_path() {
        std::env::set_current_dir("/usr").unwrap();
        let (dir, file) = resolve_file("bin/bash".as_ref()).unwrap();

        assert_eq!(dir, Path::new("/usr/bin"));
        assert_eq!(file, OsStr::new("bash"));
    }

    #[test]
    fn with_repeated_slashes() {
        let (dir, file) = resolve_file("//usr/bin///env".as_ref()).unwrap();

        assert_eq!(dir, Path::new("/usr/bin"));
        assert_eq!(file, OsStr::new("env"));
    }
}
