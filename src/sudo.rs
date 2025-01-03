use alloc::ffi::CString;
use core::convert::Infallible;

use anyhow::Result;
use nix::unistd::Uid;
use nix::unistd::execv;

/// Variables that shall be passed to the program across `run0`, if present.
const SHARED_ENVS: &[&str] = &[
    "RUST_BACKTRACE",
    "LS_COLORS",
    // https://bixense.com/clicolors/
    "NO_COLOR",
    "CLICOLOR_FORCE",
    "CLICOLOR",
];

/// Replace current process with a `run0` invocation to `program`.
///
/// On success, this function does not return.
///
/// # Errors
///
/// `execv` may fail for multiple runtime issue described in [`execv(3)`](https://man.archlinux.org/man/execv.3).
pub fn run0(program: impl IntoIterator<Item = impl Into<Vec<u8>>>) -> Result<Infallible> {
    let binary = c"/usr/bin/run0";

    let mut args = vec![binary.to_owned()];
    for &env in SHARED_ENVS {
        if std::env::var_os(env).is_some() {
            let arg = format!("--setenv={env}");
            args.push(CString::new(arg)?);
        }
    }

    args.push(c"--".to_owned());
    for arg in program {
        args.push(CString::new(arg)?);
    }

    println!("{args:?}");
    Ok(execv(binary, &args)?)
}

/// Check if current program has root privileges.
#[inline]
#[must_use]
pub fn is_root() -> bool {
    Uid::effective().is_root()
}
