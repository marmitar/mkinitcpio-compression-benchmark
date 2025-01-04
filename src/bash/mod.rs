/// Utilities for interaction with Bash.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Result, bail};
use format_bytes::{format_bytes, write_bytes};

mod byte_string;

pub use byte_string::ByteString;

/// Either a list `VARNAME=VALUE`, as understood by Bash.
pub type Environment = HashMap<ByteString, BashValue>;

/// Source a bash file and capture environment variables.
///
/// Note that this doesn't make a distinction from globallly imported variable and local variables created at source.
///
/// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
///
/// # Errors
///
/// Could fail with runtime errors or path resolution errors.
pub fn source(path: impl AsRef<Path>) -> Result<Environment> {
    fn inner(input_path: &Path) -> Result<Environment> {
        let path = input_path.canonicalize()?;
        if !path.is_file() {
            bail!("not a file: {} (resolved from {})", path.display(), input_path.display());
        }

        let (Some(dir), Some(file)) = (path.parent(), path.file_name()) else {
            bail!("invalid path: {} (resolved from {})", path.display(), input_path.display());
        };

        let command = format_bytes!(
            b"source '{}' 1>&-
            declare",
            file.as_bytes(),
        );

        parse_vars(rbash_at(&command, dir)?)
    }

    inner(path.as_ref())
}

/// Execute `mapfile` to split a string into a bash array.
///
/// Splitting is done according to `delimiter`.
///
/// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
///
/// # Errors
///
/// Could fail with runtime errors.
pub fn mapfile(delimiter: u8, content: impl AsRef<[u8]>) -> Result<Environment> {
    fn inner(delimiter: u8, content: &[u8]) -> Result<Environment> {
        let command = format_bytes!(
            b"declare -a OUTPUT
            OUTPUT=()
            INPUT={}
            mapfile -d '{}' -t OUTPUT 1>&- < <(
                printf '%s' \"${}INPUT[*]{}\"
            )
            declare",
            content,
            [delimiter],
            b"{",
            b"}",
        );

        let mut environment = parse_vars(rbash(command)?)?;
        let Some(output) = environment.remove::<[u8]>(b"OUTPUT") else {
            bail!("missing OUTPUT variable from mapfile execution");
        };

        if !output.is_array() {
            bail!("mapfile did not create a valid bash array");
        }

        output.array()
    }

    inner(delimiter, content.as_ref())
}

/// Run a restricted Bash shell at `/`.
fn rbash(commands: impl AsRef<[u8]>) -> Result<ByteString> {
    rbash_at(commands.as_ref(), Path::new("/"))
}

/// Run a restricted Bash shell at `dir`.
fn rbash_at(commands: &[u8], dir: &Path) -> Result<ByteString> {
    let mut child = Command::new("/usr/bin/bash")
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .arg("-r")
        .spawn()?;

    let Some(mut stdin) = child.stdin.take() else {
        bail!("no stdin pipe provided to communicate with bash");
    };

    write_bytes!(&mut stdin, b"set -o errexit\n")?;
    write_bytes!(&mut stdin, b"{}\n", commands)?;
    write_bytes!(&mut stdin, b"exit\n")?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let message = "bash script failed";
        let stderr = String::from_utf8_lossy(&output.stderr);

        match (output.status.code(), stderr.trim().is_empty()) {
            (Some(code), false) => bail!("{message} (status = {code}): {stderr}"),
            (Some(code), true) => bail!("{message} (status = {code})"),
            (None, false) => bail!("{message}: {stderr}"),
            (None, true) => bail!("{message}"),
        }
    }

    if !output.stderr.is_empty() {
        std::io::stderr().write_all(&output.stderr)?;
    }

    std::io::stderr().write_all(&output.stderr)?;
    Ok(output.stdout.into())
}

/// Parse a string of `VARNAME=VALUE` variables.
fn parse_vars(text: ByteString) -> Result<Environment> {
    text.data
        .into_iter()
        .as_slice()
        .split(|&ch| ch == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let Some(idx) = line.iter().position(|&ch| ch == b'=') else {
                bail!("missing variable assignment: {:?}", String::from_utf8_lossy(line));
            };

            let (varname, content) = line.split_at(idx);
            Ok((varname.into(), BashValue::new(&content[1..])))
        })
        .collect()
}

/// Either a Bash string or a Bash array.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BashValue {
    /// Source text of the value, as understood by bash.
    content: ByteString,
}

impl BashValue {
    /// Copies all bytes into a new bash value.
    #[inline]
    #[must_use]
    fn new(content: &[u8]) -> Self {
        Self {
            content: content.into(),
        }
    }

    /// Check if variable should be considered an array or a plain string.
    #[inline]
    #[must_use]
    pub const fn is_array(&self) -> bool {
        // FUTURE: self.content.data.starts_with(b"(")
        matches!(self.content.data.first().copied(), Some(b'('))
    }

    /// Reads variable as a plain string.
    ///
    /// Arrays are concatenated into a single string with spaces.
    pub fn text(&self) -> Result<ByteString> {
        let command = format_bytes!(
            b"VAR={}
            echo -n \"${}VAR[*]{}\"",
            self.content,
            b"{",
            b"}",
        );
        rbash(command)
    }

    /// Reads variable as an associative array.
    ///
    /// Doesn't work with numeric arrays.
    pub fn array(&self) -> Result<Environment> {
        let command = format_bytes!(
            b"declare -A ARR
            ARR={}
            for KEY in \"${}!ARR[@]{}\"; do
                printf '%q=%q\\n' \"$KEY\" \"${}ARR[$KEY]{}\"
            done",
            self.content,
            b"{",
            b"}",
            b"{",
            b"}",
        );

        parse_vars(rbash(command)?)
    }
}

#[cfg(test)]
mod test {
    use map_macro::hash_map;
    use pretty_assertions::{assert_eq, assert_matches};
    use tempfile::NamedTempFile;

    use super::*;

    fn utf8(bytes: impl AsRef<[u8]>) -> String {
        String::from_utf8_lossy(bytes.as_ref()).into_owned()
    }

    #[test]
    fn bash_stdout_piped() {
        let output = rbash(b"echo 'test string'").unwrap();
        assert_eq!(utf8(output), "test string\n", "echo output parsed correctly");

        let output = rbash(b"echo -n 'test string'").unwrap();
        assert_eq!(utf8(output), "test string", "echo -n doesn't have newlines");
    }

    #[test]
    fn bash_failures_handled() {
        let err = rbash(b"echo()").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("syntax error"), "syntax error captured");

        let err = rbash(b"exit 55").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("(status = 55)"), "exit code reported");

        let err = rbash(b"printf '%z'").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("missing format character"), "bash printf error");

        let err = rbash(b"whoami --wrong=arg").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("unrecognized option"), "external binary error");
    }

    #[test]
    fn text_variable_is_unescaped() {
        let var = BashValue::new(b"justASingleWord");
        assert_eq!(var.is_array(), false);
        assert_eq!(var.text().unwrap(), "justASingleWord");
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"justASingleWord"),
        });

        let var = BashValue::new(b"'text with spaces'");
        assert_eq!(var.is_array(), false);
        assert_eq!(var.text().unwrap(), "text with spaces");
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"text\\ with\\ spaces"),
        });

        let var = BashValue::new(b"$'contains\\tescapes\\n'");
        assert_eq!(var.is_array(), false);
        assert_eq!(utf8(var.text().unwrap()), "contains\tescapes\n");
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"$'contains\\tescapes\\n'"),
        });

        let var = BashValue::new(b"$'null character\\0 is ignored'");
        assert_eq!(var.is_array(), false);
        assert_eq!(utf8(var.text().unwrap()), "null character");
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"null\\ character"),
        });
    }

    #[test]
    fn associative_array_variable_is_unescaped() {
        let var = BashValue::new(b"()");
        assert_eq!(var.is_array(), true);
        assert_eq!(var.array().unwrap(), hash_map! {});
        assert_eq!(utf8(var.text().unwrap()), "");

        let var = BashValue::new(b"'()'");
        assert_eq!(var.is_array(), false);
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"\\(\\)"),
        });
        assert_eq!(utf8(var.text().unwrap()), "()");

        let var = BashValue::new(b"([0]=first [1]='second item')");
        assert_eq!(var.is_array(), true);
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"first"),
            ByteString::new("1") => BashValue::new(b"second\\ item"),
        });
        assert_eq!(utf8(var.text().unwrap()), "first second item");

        let var = BashValue::new(b"(nonAssociative)");
        assert_eq!(var.is_array(), true);
        assert_eq!(var.array().unwrap(), hash_map! {
            ByteString::new("nonAssociative") => BashValue::new(b"''"),
        });
        assert_eq!(utf8(var.text().unwrap()), "nonAssociative");
    }

    macro_rules! tmpfile {
        ($($arg:tt)*) => {{
            let mut tmp = NamedTempFile::new().expect("could not create temporary file");
            write!(tmp, $($arg)*).expect("could not write contents to temporary file");
            tmp.into_temp_path()
        }};
    }

    #[test]
    fn source_variables() {
        let tmp = tmpfile! {"
            simple='just basic text'
            var=$(echo \"hi\")

            declare -a some_array
            some_array[0]=firstItem
            some_array[1]='second\nItem'
            some_array[2]=done
        "};

        let vars = source(tmp).unwrap();
        let var = |name: &[u8]| vars.get(name).unwrap();

        assert_eq!(var(b"simple"), &BashValue::new(b"'just basic text'"));
        assert_eq!(var(b"simple").text().unwrap(), "just basic text");

        assert_eq!(var(b"var"), &BashValue::new(b"hi"));
        assert_eq!(var(b"var").text().unwrap(), "hi");

        assert_eq!(var(b"some_array"), &BashValue::new(b"([0]=\"firstItem\" [1]=$'second\\nItem' [2]=\"done\")"));
        assert_eq!(var(b"some_array").array().unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"firstItem"),
            ByteString::new("1") => BashValue::new(b"$'second\\nItem'"),
            ByteString::new("2") => BashValue::new(b"done"),
        });
        assert_eq!(var(b"some_array").text().unwrap(), "firstItem second\nItem done");
    }

    #[test]
    fn mapfile_string_to_array() {
        assert_eq!(mapfile(b' ', "\"-S string -T 'multi word text'\"").unwrap(), hash_map! {
            ByteString::new("0") => BashValue::new(b"-S"),
            ByteString::new("1") => BashValue::new(b"string"),
            ByteString::new("2") => BashValue::new(b"-T"),
            ByteString::new("3") => BashValue::new(b"\\'multi"),
            ByteString::new("4") => BashValue::new(b"word"),
            ByteString::new("5") => BashValue::new(b"text\\'"),
        });
    }
}
