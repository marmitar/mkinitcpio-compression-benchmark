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
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use mkinitcpio_compression_benchmark::mkinitcpio::{Preset, create_mock_preset};
use mkinitcpio_compression_benchmark::{UserSpec, sudo};

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
pub fn main() {
    env_logger::init();
    let cli = Cli::parse();
    let result = panic::catch_unwind(|| run(cli.clone()));

    log::debug!("recursive_chown: owner={}, path={}", cli.chown, cli.outdir.display());
    if let Err(error) = cli.chown.recursive_chown(&cli.outdir) {
        log::warn!("{error}");
    }

    match result {
        Err(error) => panic::resume_unwind(error),
        Ok(Err(error)) => log::error!("{error}"),
        Ok(Ok(())) => (),
    }
}

/// Normal execution.
///
/// # Errors
///
/// Any runtime error in the program.
fn run(cli: Cli) -> Result<()> {
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

    let mut default_config = None;
    for preset in Preset::load_default_presets()? {
        let preset = create_mock_preset(preset, &outdir, &mut default_config)?;
        log::info!("preset = {}", preset.display());
    }
    todo!("{user}");
}
