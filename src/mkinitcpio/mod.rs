//! Execution and configuration of `mkinitcpio`.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

mod config;
mod preset;

pub use config::Config;
pub use preset::Preset;

/// Run `mkinitcpio` using the provided preset file.
///
/// Return `stdout` for verbose output.
///
/// # Errors
///
/// Multiple reasons.
pub fn mkinitcpio(preset: &Path) -> Result<String> {
    let output = Command::new("/usr/bin/mkinitcpio")
        .arg("--preset")
        .arg(preset)
        .output()?;

    if !output.status.success() {
        let message = "mkinitcpio failed";
        let stderr = String::from_utf8_lossy(&output.stderr);

        match (output.status.code(), stderr.trim().is_empty()) {
            (Some(code), false) => bail!("{message} (status = {code}): {stderr}"),
            (Some(code), true) => bail!("{message} (status = {code})"),
            (None, false) => bail!("{message}: {stderr}"),
            (None, true) => bail!("{message}"),
        }
    }

    std::io::stderr().write_all(&output.stderr)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.into_owned())
}
