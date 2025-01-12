//! Utilities for interaction with Bash.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::{Result, bail};
use format_bytes::format_bytes;
use hashbrown::HashMap;

mod array;
mod exec;
mod string;

pub use array::BashArray;
pub use string::BashString;

/// List of `NAME=VALUE` variables from Bash.
pub type Environment = HashMap<BashString, BashValue>;

/// Source a bash file and capture environment variables.
///
/// Note that this doesn't make a distinction from globallly imported variable and local variables created at source.
///
/// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
///
/// # Errors
///
/// Could fail with runtime errors or path resolution errors.
pub fn source(path: &Path) -> Result<Environment> {
    let (dir, file) = exec::resolve_file(path)?;
    let command = format_bytes!(
        b"source '{}' 1>&-
        declare",
        file.as_bytes(),
    );

    let output = exec::rbash_at(&command, &dir)?;
    parse_vars(output, |key| BashString::from_escaped(key), BashValue::from_source)
}

/// Parse a string of `NAME=VALUE` variables.
fn parse_vars<K, V, C: FromIterator<(K, V)>>(
    bytes: Vec<u8>,
    parse_key: fn(&str) -> Result<K>,
    parse_value: fn(&str) -> Result<V>,
) -> Result<C> {
    String::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            log::trace!("parse_vars: {line}");
            let Some((name, value)) = line.split_once('=') else {
                bail!("missing variable assignment: {line}");
            };
            Ok((parse_key(name)?, parse_value(value)?))
        })
        .collect()
}

/// Represents a value from a variable in Bash.
#[derive(Clone, PartialEq, Eq, Hash)]
#[expect(clippy::exhaustive_enums, reason = "only two kinds of variable in Bash")]
pub enum BashValue {
    /// Simple string variable.
    String(BashString),
    /// Indexed array variable.
    Array(BashArray),
}

impl BashValue {
    /// Parses either a string or an array value from a Bash variable.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for invalid or unquoted data, and for other runtime errors in Bash.
    pub fn from_source(text: &str) -> Result<Self> {
        if array::is_array_source(text.trim()) {
            Ok(Self::Array(BashArray::new(text)?))
        } else {
            Ok(Self::String(BashString::from_escaped(text)?))
        }
    }

    /// Quoted form of the string or array.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &str {
        match self {
            Self::String(string) => string.source(),
            Self::Array(array) => array.source(),
        }
    }

    /// If this is a string value, access it.
    #[inline]
    #[must_use]
    pub const fn string(&self) -> Option<&BashString> {
        match self {
            Self::String(string) => Some(string),
            Self::Array(_) => None,
        }
    }

    /// If this is an array value, access it.
    #[inline]
    #[must_use]
    pub const fn array(&self) -> Option<&BashArray> {
        match self {
            Self::String(_) => None,
            Self::Array(array) => Some(array),
        }
    }
}

#[cfg(test)]
mod test {
    use std::io::Write;

    use pretty_assertions::assert_eq;
    use tempfile::NamedTempFile;
    use test_log::test;

    use super::*;

    #[test]
    fn text_variable_is_unescaped() {
        let var = BashValue::from_source("justASingleWord").unwrap();
        assert_eq!(var.string().unwrap(), "justASingleWord");
        assert_eq!(var.array(), None);

        let var = BashValue::from_source("'text with spaces'").unwrap();
        assert_eq!(var.string().unwrap(), "text with spaces");
        assert_eq!(var.array(), None);

        let var = BashValue::from_source("$'contains\\tescapes\\n'").unwrap();
        assert_eq!(var.string().unwrap(), "contains\tescapes\n");
        assert_eq!(var.array(), None);

        let var = BashValue::from_source("$'null character\\0 is ignored'").unwrap();
        assert_eq!(var.string().unwrap(), "null character");
        assert_eq!(var.array(), None);
    }

    #[test]
    fn associative_array_variable_is_unescaped() {
        let var = BashValue::from_source("()").unwrap();
        assert_eq!(var.array().unwrap(), [""; 0].as_slice());
        assert_eq!(var.string(), None);

        let var = BashValue::from_source("'()'").unwrap();
        assert_eq!(var.array(), None);
        assert_eq!(var.string().unwrap(), "()");

        let var = BashValue::from_source("([0]=first [1]='second item')").unwrap();
        assert_eq!(var.array().unwrap().to_concatenated_string().unwrap(), "first second item");
        assert_eq!(var.array().unwrap(), ["first", "second item"].as_slice());
        assert_eq!(var.string(), None);

        let var = BashValue::from_source("(nonAssociative)").unwrap();
        assert_eq!(var.array().unwrap(), ["nonAssociative"].as_slice());
        assert_eq!(var.string(), None);
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
            some_array[10]=done
        "};

        let vars = source(&tmp).unwrap();
        let var = |name: &str| vars.get(name.as_bytes()).unwrap();

        assert_eq!(var("simple").source(), "'just basic text'");
        assert_eq!(var("simple").string().unwrap(), "just basic text");

        assert_eq!(var("var").source(), "hi");
        assert_eq!(var("var").string().unwrap(), "hi");

        assert_eq!(var("some_array").source(), "([0]=\"firstItem\" [1]=$'second\\nItem' [10]=\"done\")");
        assert_eq!(var("some_array").array().unwrap(), ["firstItem", "second\nItem", "done",].as_slice());
    }
}
