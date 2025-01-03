use core::convert::Infallible;
use alloc::ffi::CString;

use anyhow::Result;
use nix::unistd::Uid;
use nix::unistd::execv;

const SHARED_ENVS: &[&str] = &[
    "RUST_BACKTRACE",
    // https://bixense.com/clicolors/
    "NO_COLOR",
    "CLICOLOR_FORCE",
    "CLICOLOR",
];

pub fn run0(program: impl IntoIterator<Item = impl Into<Vec<u8>>>) -> Result<Infallible> {
    let binary = c"/usr/bin/run0";

    let mut args =  vec![binary.to_owned()];
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

pub fn is_root() -> bool {
    Uid::effective().is_root()
}
