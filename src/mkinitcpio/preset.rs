//! Processing preset files for `mkinitcpio`.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::{fmt, io};

use anyhow::{Result, bail};
use format_bytes::format_bytes;

use super::Config;
use crate::bash::{self, BashArray, BashString, BashValue, Environment};

/// Parsed preset for `mkinitcpio`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Preset {
    /// Filename for the preset file.
    pub filename: BashString,
    /// Name for the preset in `PRESETS`.
    pub name: BashString,
    /// Path to the kernel version to use.
    pub kver: Option<BashString>,
    /// Path to the [`Config`] to use.
    pub config: Option<BashString>,
    /// Path to store output `initramfs` image.
    pub image: Option<BashString>,
    /// Path to store output UKI.
    pub uki: Option<BashString>,
    /// Path to store output UKI (deprecade).
    pub efi_image: Option<BashString>,
    /// Path for the microcode to be loaded (deprecated).
    pub microcode: Option<BashString>,
    /// Additional CLI options for `mkinitcpio`.
    pub options: Option<BashArray>,
}

impl Preset {
    /// Parse a single preset by `name`.
    fn parse_preset(filename: &BashString, env: &Environment, name: &BashString) -> Result<Self> {
        macro_rules! var {
            ($preset:expr, $suffix:expr) => {{
                let preset: &[u8] = $preset.as_ref();
                let suffix: &[u8] = $suffix.as_ref();
                let varname = format_bytes!(b"{}_{}", preset, suffix);
                env.get(&varname.into_boxed_slice())
            }};
        }

        fn as_string(value: &BashValue) -> Result<BashString> {
            match value {
                BashValue::String(string) => string.reescape(),
                BashValue::Array(array) => array.to_concatenated_string(),
            }
        }

        fn as_array(value: &BashValue) -> Result<BashArray> {
            match value {
                BashValue::String(string) => string.mapfile(b' ')?.reescape(),
                BashValue::Array(array) => array.reescape(),
            }
        }

        Ok(Self {
            filename: filename.clone(),
            name: name.clone(),
            kver: var!(name, "kver")
                .or_else(|| var!("ALL", "kver"))
                .map(as_string)
                .transpose()?,
            config: var!(name, "config")
                .or_else(|| var!("ALL", "config"))
                .map(as_string)
                .transpose()?,
            uki: var!(name, "uki").map(as_string).transpose()?,
            efi_image: var!(name, "efi_image").map(as_string).transpose()?,
            image: var!(name, "image").map(as_string).transpose()?,
            options: var!(name, "options").map(as_array).transpose()?,
            microcode: var!(name, "microcode")
                .or_else(|| var!("ALL", "microcode"))
                .map(as_string)
                .transpose()?,
        })
    }

    /// Parse all `PRESETS` defined in a file.
    ///
    /// # Errors
    ///
    /// File cannot be read, or another runtime error.
    pub fn load_preset(preset_path: &Path) -> Result<Vec<Self>> {
        let Some(filename) = preset_path.file_stem() else {
            bail!("missing filename for preset: {}", preset_path.display());
        };
        let filename = BashString::from_raw(filename.as_bytes())?;

        let env = bash::source(preset_path)?;
        let Some(presets) = env.get(b"PRESETS".as_slice()) else {
            bail!("missing PRESETS array");
        };

        let presets = match presets {
            BashValue::Array(array) => array.reescape()?,
            BashValue::String(string) => std::iter::once(string.reescape()?).collect(),
        };

        presets
            .into_values()
            .map(|name| Self::parse_preset(&filename, &env, &name))
            .collect()
    }

    //// Load all `*.preset` files in a directory.
    ///
    /// Not recursive.
    ///
    /// # Errors
    ///
    /// File or directory cannot be read, or another runtime error.
    pub fn load_all_presets(folder: &Path) -> Result<Vec<Self>> {
        let mut presets = Vec::new();
        for entry in std::fs::read_dir(folder)? {
            let preset_path = entry?.path();
            if preset_path.extension() == Some("preset".as_ref()) {
                presets.extend(Self::load_preset(&preset_path)?);
            }
        }
        Ok(presets)
    }

    //// Load all `*.preset` files in the default directory.
    ///
    /// Presets searched at `/etc/mkinitcpio.d/*.preset`.
    ///
    /// # Errors
    ///
    /// File or directory cannot be read, or another runtime error.
    pub fn load_default_presets() -> Result<Vec<Self>> {
        Self::load_all_presets(Path::new("/etc/mkinitcpio.d"))
    }

    /// Saves current preset to the specified path.
    ///
    /// # Errors
    ///
    /// IO and other runtime errors.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(dir) {
                log::info!("create_dir_all: error={error}");
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error.into());
                }
            }
        }

        std::fs::write(path, self.to_string())?;
        Ok(())
    }

    /// Load the configuration for this preset, if any.
    ///
    /// # Errors
    ///
    /// IO and other runtime errors.
    pub fn load_config(&self) -> Result<Option<Config>> {
        self.config
            .as_ref()
            .map(BashString::as_path)
            .map(Config::load_config)
            .transpose()
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        macro_rules! write_var {
            ($name:ident) => {
                if let Some(value) = &self.$name {
                    writeln!(f, "{}_{}={}", self.name.source(), stringify!($name), value.source())
                } else {
                    Ok(())
                }
            };
        }

        writeln!(f, "PRESETS=({})", self.name.source())?;
        write_var!(kver)?;
        write_var!(config)?;
        write_var!(image)?;
        write_var!(uki)?;
        write_var!(efi_image)?;
        write_var!(microcode)?;
        write_var!(options)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;
    use test_log::test;

    use super::*;

    fn example_preset() -> TempDir {
        let contents = "
# mkinitcpio preset file for the 'linux' package

#ALL_config=\"/etc/mkinitcpio.conf\"
ALL_kver=\"/boot/vmlinuz-linux\"

PRESETS=('default' 'fallback')

#default_config=\"/etc/mkinitcpio.conf\"
default_image=\"/boot/initramfs-linux.img\"
#default_uki=\"/efi/EFI/Linux/arch-linux.efi\"
default_options=\"--splash /usr/share/systemd/bootctl/splash-arch.bmp\"

#fallback_config=\"/etc/mkinitcpio.conf\"
fallback_image=\"/boot/initramfs-linux-fallback.img\"
#fallback_uki=\"/efi/EFI/Linux/arch-linux-fallback.efi\"
fallback_options=\"-S autodetect\"
";

        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("example.preset"), contents).unwrap();
        dir
    }

    #[test]
    pub fn loads_and_save() {
        let preset_dir = example_preset();

        let mut presets = Preset::load_all_presets(preset_dir.path()).unwrap().into_iter();
        let default = presets.next().unwrap();
        let fallback = presets.next().unwrap();
        assert_eq!(presets.next(), None);
        assert_eq!(default.filename, fallback.filename);

        assert_eq!(default.name, "default");
        assert_eq!(default.kver.as_ref().unwrap(), "/boot/vmlinuz-linux");
        assert_eq!(default.config.as_ref(), None);
        assert_eq!(default.image.as_ref().unwrap(), "/boot/initramfs-linux.img");
        assert_eq!(default.uki.as_ref(), None);
        assert_eq!(default.efi_image.as_ref(), None);
        assert_eq!(default.microcode.as_ref(), None);
        assert_eq!(*default.options.as_ref().unwrap(), ["--splash", "/usr/share/systemd/bootctl/splash-arch.bmp"]);

        assert_eq!(fallback.name, "fallback");
        assert_eq!(fallback.kver.as_ref().unwrap(), "/boot/vmlinuz-linux");
        assert_eq!(fallback.config.as_ref(), None);
        assert_eq!(fallback.image.as_ref().unwrap(), "/boot/initramfs-linux-fallback.img");
        assert_eq!(fallback.uki.as_ref(), None);
        assert_eq!(fallback.efi_image.as_ref(), None);
        assert_eq!(fallback.microcode.as_ref(), None);
        assert_eq!(*fallback.options.as_ref().unwrap(), ["-S", "autodetect"]);

        let output_path = preset_dir.path().join("output.preset");
        default.save_to(&output_path).unwrap();
        let bytes = std::fs::read(output_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert_eq!(
            content.trim(),
            "
PRESETS=(default)
default_kver=/boot/vmlinuz-linux
default_image=/boot/initramfs-linux.img
default_options=(--splash /usr/share/systemd/bootctl/splash-arch.bmp)
"
            .trim()
        );
    }
}
