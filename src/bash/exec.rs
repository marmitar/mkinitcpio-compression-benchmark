use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use format_bytes::{format_bytes, write_bytes};

/// Run a restricted Bash shell at `/`.
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

    write_bytes!(&mut stdin, b"set -o errexit\n")?;
    write_bytes!(&mut stdin, b"{}\n", commands)?;
    write_bytes!(&mut stdin, b"exit\n")?;
    core::mem::drop(stdin);

    let output = child.wait_with_output()?;

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

    std::io::stderr().write_all(&output.stderr)?;
    Ok(output.stdout)
}

pub fn rbash_with_output(commands: impl AsRef<[u8]>) -> Result<String> {
    rbash_with_output_at(commands.as_ref(), Path::new("/"))
}

pub fn rbash_with_output_at(commands: &[u8], dir: &Path) -> Result<String> {
    let commands_with_output = format_bytes!(
        b"{}
        declare | grep -E '^OUTPUT='",
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

    if !path.is_file() {
        bail!("not a file: {} (resolved from {})", path.display(), input_path.display());
    }
    let (Some(dir), Some(file)) = (path.parent(), path.file_name()) else {
        bail!("invalid path: {} (resolved from {})", path.display(), input_path.display());
    };
    Ok((dir.to_owned(), file.to_owned()))
}

#[cfg(test)]
mod test {
    use std::ffi::OsStr;

    use pretty_assertions::{assert_eq, assert_matches};

    use super::*;

    fn utf8(bytes: impl AsRef<[u8]>) -> String {
        String::from_utf8_lossy(bytes.as_ref()).into_owned()
    }

    #[test]
    fn bash_stdout_piped() {
        let output = rbash(b"echo 'test string'").unwrap();
        assert_eq!(utf8(output), "test string\n", "echo output parsed correctly");

        let output = rbash(b"echo -n 'test string'").unwrap();
        assert_eq!(utf8(output), "test string", "echo -n doesn't have newlines");
    }

    #[test]
    fn bash_failures_handled() {
        let err = rbash(b"echo()").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("syntax error"), "syntax error captured");

        let err = rbash(b"exit 55").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("(status = 55)"), "exit code reported");

        let err = rbash(b"printf '%z'").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("missing format character"), "bash printf error");

        let err = rbash(b"whoami --wrong=arg").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("unrecognized option"), "external binary error");
    }

    #[test]
    fn file_is_resolved() {
        std::env::set_current_dir("/usr").unwrap();
        let (dir, file) = resolve_file("bin/bash".as_ref()).unwrap();

        assert_eq!(dir, Path::new("/usr/bin"));
        assert_eq!(file, OsStr::new("bash"));
    }
}
