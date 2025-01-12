//! Execution and configuration of `mkinitcpio`.

use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};

mod config;
mod preset;

pub use config::Config;
pub use preset::Preset;

use crate::utils::strings::lines;

/// Run `mkinitcpio` using the provided preset file.
///
/// Return `stdout` for verbose output.
///
/// # Errors
///
/// Multiple reasons.
pub fn mkinitcpio(preset: &Path) -> Result<()> {
    log::trace!("mkinitcpio: preset={}", preset.display());
    let output = Command::new("/usr/bin/mkinitcpio")
        .arg("--preset")
        .arg(preset)
        .output()?;

    log::trace!(
        "mkinitcpio: exit={}, #lines stdout={}, #lines stderr={}",
        output.status,
        lines(&output.stdout).count(),
        lines(&output.stderr).count()
    );
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

    for line in lines(&output.stderr) {
        log::error!("mkinitcpio: {}", line.escape_ascii());
    }
    for line in lines(&output.stdout) {
        log::info!("mkinitcpio: {}", line.escape_ascii());
    }
    Ok(())
}
