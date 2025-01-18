//! `mkinitcpio-compression-benchmark` binary.

// Additional Errors
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::all)]
#![deny(clippy::allow_attributes)]
#![deny(clippy::allow_attributes_without_reason)]
#![deny(clippy::lossy_float_literal)]
// More Warnings
#![warn(clippy::alloc_instead_of_core)]
#![warn(clippy::as_underscore)]
#![warn(clippy::clone_on_ref_ptr)]
#![warn(clippy::create_dir)]
#![warn(clippy::decimal_literal_representation)]
#![warn(clippy::empty_drop)]
#![warn(clippy::exhaustive_enums)]
#![warn(clippy::exit)]
#![warn(clippy::filetype_is_file)]
#![warn(clippy::float_cmp_const)]
#![warn(clippy::fn_to_numeric_cast_any)]
#![warn(clippy::format_push_string)]
#![warn(clippy::if_then_some_else_none)]
#![warn(clippy::infinite_loop)]
#![warn(clippy::integer_division_remainder_used)]
#![warn(clippy::map_err_ignore)]
#![warn(clippy::map_with_unused_argument_over_ranges)]
#![warn(clippy::mem_forget)]
#![warn(clippy::missing_assert_message)]
#![warn(clippy::missing_docs_in_private_items)]
#![warn(clippy::mixed_read_write_in_expression)]
#![warn(clippy::multiple_inherent_impl)]
#![warn(clippy::multiple_unsafe_ops_per_block)]
#![warn(clippy::mutex_atomic)]
#![warn(clippy::needless_raw_strings)]
#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
#![warn(clippy::non_zero_suggestions)]
#![warn(clippy::panic_in_result_fn)]
#![warn(clippy::redundant_type_annotations)]
#![warn(clippy::ref_patterns)]
#![warn(clippy::rest_pat_in_fully_bound_structs)]
#![warn(clippy::self_named_module_files)]
#![warn(clippy::semicolon_outside_block)]
#![warn(clippy::str_to_string)]
#![warn(clippy::string_to_string)]
#![warn(clippy::tests_outside_test_module)]
#![warn(clippy::try_err)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::unnecessary_safety_comment)]
#![warn(clippy::unnecessary_safety_doc)]
#![warn(clippy::unneeded_field_pattern)]
#![warn(clippy::unseparated_literal_suffix)]
#![warn(clippy::unused_result_ok)]
#![warn(clippy::unwrap_in_result)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::wildcard_enum_match_arm)]
#![warn(clippy::unnecessary_self_imports)]

use std::os::unix::ffi::OsStringExt;
use std::panic;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Result;
use byte_unit::UnitType;
use clap::Parser;

mod bash;
mod measure;
mod mkinitcpio;
mod sudo;
mod user_spec;
mod utils;

use crate::measure::{Stats, exec};
use crate::mkinitcpio::{Config, Preset, create_mock_preset, mkinitcpio};
use crate::user_spec::UserSpec;

/// A compression method to be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Compression {
    /// Unique method name.
    name: &'static str,
    /// Extension for compressed file.
    extension: &'static str,
    /// Compress a file.
    compress: fn(path: &Path) -> Result<Stats>,
    /// Decompress a file.
    decompress: fn(path: &Path) -> Result<Stats>,
}

/// List of compression methods to test.
const COMPRESSION: &[Compression] = &[
    Compression {
        name: "lz4-fast",
        extension: ".lz4",
        compress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), "-12".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
    Compression {
        name: "lz4-norm",
        extension: ".lz4",
        compress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
    Compression {
        name: "lz4-high",
        extension: ".lz4",
        compress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), "--fast=12".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/lz4", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
    Compression {
        name: "zstd-fast",
        extension: ".zst",
        compress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-1".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
    Compression {
        name: "zstd-norm",
        extension: ".zst",
        compress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-5".as_ref(), "--long".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
    Compression {
        name: "zstd-high",
        extension: ".zst",
        compress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-19".as_ref(), "--long".as_ref(), path.as_os_str()]),
        decompress: |path| exec("/usr/bin/zstdmt", ["-v".as_ref(), "-d".as_ref(), path.as_os_str()]),
    },
];

/// Run some benchmarks on mkinitcpio compression and decompression algorithms
#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Directory to place output files.
    #[arg(short, long, default_value = "./output", required = false)]
    outdir: PathBuf,

    /// Set owner for output directories and files.
    #[arg(short, long, value_name = "[OWNER][:[GROUP]]", default_value = ":", required = false)]
    chown: UserSpec,
}

/// Binary entrypoint.
#[must_use]
pub fn main() -> ExitCode {
    env_logger::init();
    let cli = Cli::parse();
    let result = panic::catch_unwind(|| run(cli.clone()));

    log::debug!("recursive_chown: owner={}, path={}", cli.chown, cli.outdir.display());
    if let Err(error) = cli.chown.recursive_chown(&cli.outdir) {
        log::warn!("{error}");
    }

    result
        .unwrap_or_else(|error| panic::resume_unwind(error))
        .unwrap_or_else(|error| {
            log::error!("{error}");
            ExitCode::FAILURE
        })
}

/// Normal execution.
///
/// # Errors
///
/// Any runtime error in the program.
fn run(cli: Cli) -> Result<ExitCode> {
    let user = cli.chown;
    let outdir = std::path::absolute(cli.outdir)?;
    let current_user = UserSpec::current_user()?;

    log::debug!("current user = {}", current_user.to_spec());
    log::debug!("outdir = {}", outdir.display());
    log::debug!("chown = {}", user.to_spec());

    if !sudo::is_root() {
        log::info!("program requires root to access mkinitcpio");

        let target_user = UserSpec {
            owner: user.owner.or(current_user.owner),
            group: user.group.or(current_user.group),
        };

        let program = std::env::current_exe()?;
        sudo::run0([
            program.into_os_string().into_vec(),
            format!("--chown={:+}", target_user.to_numeric_spec()).into(),
            ["--outdir=".into(), outdir.into_os_string().into_vec()].concat(),
        ])?;
        unreachable!("exec run0 should either replace the process or fail, ending current execution here");
    }

    let mut exit_code = ExitCode::SUCCESS;
    let mut default_config = None;
    for preset in Preset::load_default_presets()? {
        if let Err(error) = preset_stats(preset, &outdir, &mut default_config) {
            log::error!("preset_stats: {error}");
            exit_code = ExitCode::FAILURE;
        }
    }
    Ok(exit_code)
}

/// Measure and display preset statistics.
fn preset_stats(preset: Preset, output_dir: &Path, default_config: &mut Option<Config>) -> Result<()> {
    let name = preset.name.to_utf8_lossy().into_owned();

    let start_time = Instant::now();
    let (preset, image, uki) = create_mock_preset(preset, output_dir, default_config)?;
    log::debug!("create_mock_preset: elapsed={:?}, preset={preset:?}", start_time.elapsed());

    let stats = mkinitcpio(&preset)?;
    log_stats(&name, &stats);

    for (idx, compression) in COMPRESSION.iter().enumerate() {
        log::debug!("preset_stats: idx={idx}, compression={compression:?}");

        for (tag, img) in [("img", &image), ("uki", &uki)] {
            let target_image = with_extension(img, &format!(".{idx}"));
            log::debug!("preset_stats: target_image={}", target_image.display());

            std::fs::copy(&image, &target_image)?;
            let stats = (compression.compress)(&target_image)?;
            log_stats(&format!("{name}/{}/{tag}/c", compression.name), &stats);

            std::fs::remove_file(&target_image)?;
            let stats = (compression.decompress)(&with_extension(&target_image, compression.extension))?;
            log_stats(&format!("{name}/{}/{tag}/d", compression.name), &stats);
        }
    }
    Ok(())
}

/// Adds string to path.
fn with_extension(path: &Path, extension: &str) -> PathBuf {
    let mut buf = path.as_os_str().to_owned();
    buf.push(extension);
    buf.into()
}

/// Display statistics.
fn log_stats(name: &str, stats: &Stats) {
    log::info!("{name}: Real time: {:?}", stats.real_time());
    log::info!(
        "{name}: Virtual time: {:?} (usr: {:?}) (sys: {:?})",
        stats.virtual_time(),
        stats.user_time(),
        stats.system_time()
    );
    log::info!("{name}: Maximum memory: {}", stats.max_rss().get_appropriate_unit(UnitType::Decimal));
    log::info!("{name}: Page faults: (minor={}, major={})", stats.minor_page_faults(), stats.major_page_faults());
    log::info!("{name}: Block operations: (input={}, output={})", stats.input_blocked(), stats.output_blocked());
    log::info!(
        "{name}: Context switches: (voluntary={}, involuntary={})",
        stats.num_vol_ctx_sw(),
        stats.num_inv_ctx_sw()
    );
}
