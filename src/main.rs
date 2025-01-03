// Additional Errors
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::all)]
#![deny(clippy::allow_attributes)]
#![deny(clippy::allow_attributes_without_reason)]
#![deny(clippy::lossy_float_literal)]
#![deny(clippy::std_instead_of_alloc)]
#![deny(clippy::std_instead_of_core)]
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

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use init_compression_benchmark::{UserSpec, sudo};

/// TODO
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Name of the person ?
    #[arg(short, long, default_value = "")]
    chown: UserSpec,

    /// Number of times to greet
    #[arg(short, long, default_value = "output/")]
    outdir: PathBuf,
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let outdir = std::path::absolute(cli.outdir)?;
    let chown = cli.chown;

    if !sudo::is_root() {
        println!("program requires root to access mkinitcpio");

        let program = std::env::current_exe()?;

        let current_user = UserSpec::current_user()?;
        let target_user = UserSpec {
            owner: chown.owner.or(current_user.owner),
            group: chown.group.or(current_user.group),
        };

        sudo::run0([
            program.into_os_string().into_encoded_bytes(),
            format!("--chown={}", target_user.to_spec()).into(),
            ["--outdir=".into(), outdir.into_os_string().into_encoded_bytes()].concat(),
        ])?;
        unreachable!("exec run0 should either replace the process or fail, ending current execution here");
    }

    todo!("{chown}");
}
