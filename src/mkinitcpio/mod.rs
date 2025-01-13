//! Execution and configuration of `mkinitcpio`.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::bash::BashString;
use crate::utils::command;

mod config;
mod preset;

pub use config::Config;
pub use preset::Preset;

/// Create a mock preset at `output_dir`.
///
/// Returns the path to the new preset file.
///
/// # Errors
///
/// Multiple reasons.
pub fn create_mock_preset(
    mut preset: Preset,
    output_dir: &Path,
    default_config: &mut Option<Config>,
) -> Result<PathBuf> {
    log::trace!("create_mock_preset: preset={}, output_dir={}", preset.name, output_dir.display());
    let preset_dir = output_dir
        .join(preset.filename.as_path().with_extension(""))
        .join(preset.name.as_path());
    cleanup(&preset_dir)?;
    create_dir(&preset_dir)?;

    let mut preset_config = preset.load_config()?;
    log::debug!(
        "create_mock_preset: preset_config={:?}, default_config={:?}",
        preset_config.is_some(),
        default_config.is_some()
    );
    let config = match (&mut preset_config, default_config) {
        (Some(config), _) | (None, Some(config)) => config,
        (None, config @ None) => config.get_or_insert(Config::load_default()?),
    };

    let config_file = preset_dir.join("mkinitcpio.conf");
    config.compression.replace(BashString::from_raw(*b"cat")?);
    config.compression_options.take();
    log::trace!("create_mock_preset: config_file={}", config_file.display());
    config.save_to(&config_file)?;

    preset.config.replace(BashString::from_path(config_file)?);
    preset
        .image
        .replace(BashString::from_path(preset_dir.join("test.img"))?);
    preset.uki.replace(BashString::from_path(preset_dir.join("test.efi"))?);
    preset.efi_image.take();

    let preset_file = preset_dir.join(preset.filename.as_path()).with_extension("preset");
    log::trace!("create_mock_preset: preset_file={}", preset_file.display());
    preset.save_to(&preset_file)?;

    Ok(preset_file)
}

/// Create directory recursively, if necessary.
///
/// # Errors
///
/// Same as [`std::fs::create_dir_all`], except that [`ErrorKind::AlreadyExists`] is ignored.
fn create_dir(at: &Path) -> Result<()> {
    if let Err(error) = std::fs::create_dir_all(at) {
        if error.kind() == ErrorKind::AlreadyExists {
            log::debug!("create_dir_all: at={}, error={error}", at.display());
        } else {
            log::warn!("create_dir_all: at={}, error={error}", at.display());
            return Err(error.into());
        }
    }

    Ok(())
}

/// Remove directory or file recursively, if necessary.
///
/// # Errors
///
/// Same as [`std::fs::remove_dir_all`], except that [`ErrorKind::NotFound`] is ignored.
fn cleanup(dir: &Path) -> Result<()> {
    match dir.metadata() {
        Ok(metadata) if metadata.is_dir() => {
            log::debug!("cleanup: dir={}, is_dir=true", dir.display());
            std::fs::remove_dir_all(dir)?;
        }
        Ok(_) => {
            log::debug!("cleanup: dir={}, is_dir=false", dir.display());
            std::fs::remove_file(dir)?;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            log::debug!("cleanup: dir={}, error={error}", dir.display());
        }
        Err(error) => {
            log::warn!("cleanup: dir={}, error={error}", dir.display());
            return Err(error.into());
        }
    }
    Ok(())
}

/// Run `mkinitcpio` using the provided preset file.
///
/// Return `stdout` for verbose output.
///
/// # Errors
///
/// Multiple reasons.
pub fn mkinitcpio(preset: &Path) -> Result<()> {
    log::trace!("mkinitcpio: preset={}", preset.display());
    let output = command::command("/usr/bin/mkinitcpio", ["--preset".as_ref(), preset.as_os_str()]).output()?;
    command::check("mkinitcpio", output, true)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use test_log::test;

    use crate::mkinitcpio::{cleanup, create_dir};

    #[test]
    fn recursive_create_and_cleanup() {
        log::set_max_level(log::LevelFilter::max());

        let dir = tempdir().unwrap();
        let path = dir.path().to_owned();

        assert!(path.is_dir());

        cleanup(&path).unwrap();
        assert!(!path.is_dir());
        assert!(!path.is_file());

        drop(dir);
        assert!(!path.is_dir());
        assert!(!path.is_file());

        cleanup(&path).unwrap();
        assert!(!path.is_dir());
        assert!(!path.is_file());

        create_dir(&path).unwrap();
        assert!(path.is_dir());

        create_dir(&path).unwrap();
        assert!(path.is_dir());

        cleanup(&path).unwrap();
        assert!(!path.is_dir());
        assert!(!path.is_file());
    }
}
