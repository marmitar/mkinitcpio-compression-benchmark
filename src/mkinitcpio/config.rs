//! Processing config files for `mkinitcpio`.

use std::fmt;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use tempfile::{NamedTempFile, TempPath};

use crate::bash::{self, BashArray, BashString, BashValue};

/// Parsed configuration file for `mkinitcpio`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    /// List of modules to be included in the initramfs, regardless of hooks detection.
    pub modules: Option<BashArray>,
    /// List of binaries to be included in the initramfs, regardless of hooks detection.
    pub binaries: Option<BashArray>,
    /// List of additional files to be loaded in the initramfs.
    pub files: Option<BashArray>,
    /// Hooks to run during initramfs creation or loading.
    pub hooks: Option<BashArray>,
    /// Compression algorithm for initramfs.
    pub compression: Option<BashString>,
    /// Command line options for the compression algorithm.
    pub compression_options: Option<BashArray>,
    /// Decompress loadable kernel modules and their firmware during initramfs creation.
    pub module_decompress: Option<BashString>,
}

impl Config {
    /// Load a configuration at the specified path.
    ///
    /// # Errors
    ///
    /// Invalid configuration or runtime errors.
    pub fn load_config(config_path: &Path) -> Result<Self> {
        fn as_string(value: BashValue) -> Result<BashString> {
            let string = match value {
                BashValue::String(string) => string,
                BashValue::Array(array) => array.to_concatenated_string()?,
            };
            string.reescape()
        }

        fn as_array(value: BashValue) -> Result<BashArray> {
            let array = match value {
                BashValue::String(string) => string.arrayize()?,
                BashValue::Array(array) => array,
            };
            array.reescape()
        }

        let mut env = bash::source(config_path)?;
        let mut var = move |name: &str| env.remove(name.as_bytes());
        Ok(Self {
            modules: var("MODULES").map(as_array).transpose()?,
            binaries: var("BINARIES").map(as_array).transpose()?,
            files: var("FILES").map(as_array).transpose()?,
            hooks: var("HOOKS").map(as_array).transpose()?,
            compression: var("COMPRESSION").map(as_string).transpose()?,
            compression_options: var("COMPRESSION_OPTIONS").map(as_array).transpose()?,
            module_decompress: var("MODULES_DECOMPRESS").map(as_string).transpose()?,
        })
    }

    /// Load a configuration at the default path.
    ///
    /// Includes `/etc/mkinitcpio.conf` and drop-ins, `/etc/mkinitcpio.conf.d/*.conf`.
    ///
    /// # Errors
    ///
    /// Invalid configuration or runtime errors.
    #[inline]
    pub fn load_default() -> Result<Self> {
        Self::load_config(&default_config()?)
    }

    /// Saves current configuration to the specified path.
    ///
    /// # Errors
    ///
    /// IO and other runtime errors.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            super::create_dir(dir)?;
        }

        std::fs::write(path, self.to_string())?;
        Ok(())
    }
}

/// Default configuration, with drop-ins included.
///
/// This is a temporary file after concatenating all drop-ins, and will be removed at drop. This is similar to what
/// `mkinitcpio` does.
fn default_config() -> Result<TempPath> {
    let mut output = NamedTempFile::new()?;
    let mut append = |file: &Path| {
        let data = std::fs::read(file)?;
        output.write_all(&data)?;
        output.write_all(b"\n")?;
        anyhow::Ok(())
    };

    append("/etc/mkinitcpio.conf".as_ref())?;
    if let Ok(drop_ins) = std::fs::read_dir("/etc/mkinitcpio.conf.d/") {
        for config in drop_ins {
            let drop_in_path = config?.path();
            if drop_in_path.extension() == Some("conf".as_ref()) {
                append(drop_in_path.as_path())?;
            }
        }
    }
    Ok(output.into_temp_path())
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        macro_rules! write_var {
            ($name:ident = $var:expr) => {
                if let Some(value) = &$var {
                    writeln!(f, "{}={}", stringify!($name), value.source())
                } else {
                    Ok(())
                }
            };
        }

        write_var!(MODULES = self.modules)?;
        write_var!(BINARIES = self.binaries)?;
        write_var!(FILES = self.files)?;
        write_var!(HOOKS = self.hooks)?;
        write_var!(COMPRESSION = self.compression)?;
        write_var!(COMPRESSION_OPTIONS = self.compression_options)?;
        write_var!(MODULES_DECOMPRESS = self.module_decompress)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::*;

    fn example_config() -> TempPath {
        let contents = "
# MODULES
# The following modules are loaded before any boot hooks are
# run.  Advanced users may wish to specify all system modules
# in this array.  For instance:
#     MODULES=(usbhid xhci_hcd)
MODULES=(amdgpu nvidia-drm)
MODULES+=(i915)

# BINARIES
# This setting includes any additional binaries a given user may
# wish into the CPIO image.  This is run last, so it may be used to
# override the actual binaries included by a given hook
# BINARIES are dependency parsed, so you may safely ignore libraries
BINARIES=()

# FILES
# This setting is similar to BINARIES above, however, files are added
# as-is and are not parsed in any way.  This is useful for config files.
FILES=(/usr/lib/firmware/edid/custom-edid.bin)


# HOOKS
# This is the most important setting in this file.  The HOOKS control the
# modules and scripts added to the image, and what happens at boot time.
# Order is important, and it is recommended that you do not change the
# order in which HOOKS are added.  Run 'mkinitcpio -H <hook name>' for
# help on a given hook.
# 'base' is _required_ unless you know precisely what you are doing.
# 'udev' is _required_ in order to automatically load modules
# 'filesystems' is _required_ unless you specify your fs modules in MODULES
# Examples:
##   This setup specifies all modules in the MODULES setting above.
##   No RAID, lvm2, or encrypted root is needed.
#    HOOKS=(base)
#
##   This setup will autodetect all modules for your system and should
##   work as a sane default
#    HOOKS=(base udev autodetect modconf block filesystems fsck)
#
##   This setup will generate a 'full' image which supports most systems.
##   No autodetection is done.
#    HOOKS=(base udev modconf block filesystems fsck)
#
##   This setup assembles a mdadm array with an encrypted root file system.
##   Note: See 'mkinitcpio -H mdadm_udev' for more information on RAID devices.
#    HOOKS=(base udev modconf keyboard keymap consolefont block mdadm_udev encrypt filesystems fsck)
#
##   This setup loads an lvm2 volume group.
#    HOOKS=(base udev modconf block lvm2 filesystems fsck)
#
##   This will create a systemd based initramfs which loads an encrypted root filesystem.
#    HOOKS=(base systemd autodetect modconf kms keyboard sd-vconsole sd-encrypt block filesystems fsck)
#
##   NOTE: If you have /usr on a separate partition, you MUST include the
#    usr and fsck hooks.
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block filesystems fsck)

# COMPRESSION
# Use this to compress the initramfs image. By default, zstd compression
# is used for Linux ≥ 5.9 and gzip compression is used for Linux < 5.9.
# Use 'cat' to create an uncompressed image.
COMPRESSION=\"zstd\"
#COMPRESSION=\"gzip\"
#COMPRESSION=\"bzip2\"
#COMPRESSION=\"lzma\"
#COMPRESSION=\"xz\"
#COMPRESSION=\"lzop\"
#COMPRESSION=\"lz4\"
#COMPRESSION=\"cat\"

# COMPRESSION_OPTIONS
# Additional options for the compressor
COMPRESSION_OPTIONS=(-v -5 --long)

# MODULES_DECOMPRESS
# Decompress loadable kernel modules and their firmware during initramfs
# creation. Switch (yes/no).
# Enable to allow further decreasing image size when using high compression
# (e.g. xz -9e or zstd --long --ultra -22) at the expense of increased RAM usage
# at early boot.
# Note that any compressed files will be placed in the uncompressed early CPIO
# to avoid double compression.
MODULES_DECOMPRESS=\"yes\"
";

        let mut tmp = NamedTempFile::with_suffix(".conf").unwrap();
        tmp.write_all(contents.as_bytes()).unwrap();
        tmp.into_temp_path()
    }

    #[test]
    pub fn loads_and_save() {
        let config_path = example_config();

        let config = Config::load_config(&config_path).unwrap();

        assert_eq!(*config.modules.as_ref().unwrap(), ["amdgpu", "nvidia-drm", "i915"]);
        assert_eq!(*config.binaries.as_ref().unwrap(), [""; 0]);
        assert_eq!(*config.files.as_ref().unwrap(), ["/usr/lib/firmware/edid/custom-edid.bin"]);
        assert_eq!(*config.hooks.as_ref().unwrap(), [
            "base",
            "udev",
            "autodetect",
            "microcode",
            "modconf",
            "kms",
            "keyboard",
            "keymap",
            "consolefont",
            "block",
            "filesystems",
            "fsck"
        ]);
        assert_eq!(config.compression.as_ref().unwrap(), "zstd");
        assert_eq!(*config.compression_options.as_ref().unwrap(), ["-v", "-5", "--long"]);
        assert_eq!(config.module_decompress.as_ref().unwrap(), "yes");

        config.save_to(&config_path).unwrap();
        let bytes = std::fs::read(config_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert_eq!(
            content.trim(),
            "
MODULES=(amdgpu nvidia-drm i915)
BINARIES=()
FILES=(/usr/lib/firmware/edid/custom-edid.bin)
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block filesystems fsck)
COMPRESSION=zstd
COMPRESSION_OPTIONS=(-v -5 --long)
MODULES_DECOMPRESS=yes
"
            .trim()
        );
    }
}
