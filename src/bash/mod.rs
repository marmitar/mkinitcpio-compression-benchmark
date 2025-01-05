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

/// Source a bash file and capture environment variables.
///
/// Note that this doesn't make a distinction from globallly imported variable and local variables created at source.
///
/// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
///
/// # Errors
///
/// Could fail with runtime errors or path resolution errors.
pub fn source(path: &Path) -> Result<HashMap<BashString, BashVariable>> {
    let (dir, file) = exec::resolve_file(path)?;
    let command = format_bytes!(
        b"source '{}' 1>&-
        declare",
        file.as_bytes(),
    );

    let output = exec::rbash_at(&command, &dir)?;
    parse_vars(output, BashVariable::from_source)
}

/// Parse a string of `VARNAME=VALUE` variables.
fn parse_vars<T, C: FromIterator<(BashString, T)>>(bytes: Vec<u8>, parse: fn(&str) -> Result<T>) -> Result<C> {
    String::from_utf8(bytes)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let Some((name, value)) = line.split_once('=') else {
                bail!("missing variable assignment: {line}");
            };
            Ok((BashString::from_escaped(name)?, parse(value)?))
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "only two kinds of variable in Bash")]
pub enum BashVariable {
    String(BashString),
    Array(BashArray),
}

impl BashVariable {
    pub fn from_source(text: &str) -> Result<Self> {
        if array::is_array_source(text.trim()) {
            Ok(Self::Array(BashArray::new(text)?))
        } else {
            Ok(Self::String(BashString::from_escaped(text)?))
        }
    }

    pub fn to_bash_string(&self) -> Result<BashString> {
        match self {
            Self::String(string) => Ok(string.clone()),
            Self::Array(array) => array.to_bash_string(),
        }
    }
}

// #[cfg(test)]
// mod test {
//     use std::io::Write;

//     use map_macro::hashbrown::hash_map;
//     use pretty_assertions::assert_eq;
//     use tempfile::NamedTempFile;

//     use super::*;

//     #[test]
//     fn text_variable_is_unescaped() {
//         let var = BashValue::new(b"justASingleWord");
//         assert_eq!(var.is_array(), false);
//         assert_eq!(var.text().unwrap(), "justASingleWord");
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"justASingleWord"),
//         });

//         let var = BashValue::new(b"'text with spaces'");
//         assert_eq!(var.is_array(), false);
//         assert_eq!(var.text().unwrap(), "text with spaces");
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"text\\ with\\ spaces"),
//         });

//         let var = BashValue::new(b"$'contains\\tescapes\\n'");
//         assert_eq!(var.is_array(), false);
//         assert_eq!(var.text().unwrap(), "contains\tescapes\n");
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"$'contains\\tescapes\\n'"),
//         });

//         let var = BashValue::new(b"$'null character\\0 is ignored'");
//         assert_eq!(var.is_array(), false);
//         assert_eq!(var.text().unwrap(), "null character");
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"null\\ character"),
//         });
//     }

//     #[test]
//     fn associative_array_variable_is_unescaped() {
//         let var = BashValue::new(b"()");
//         assert_eq!(var.is_array(), true);
//         assert_eq!(var.array().unwrap(), hash_map! {});
//         assert_eq!(var.text().unwrap(), "");

//         let var = BashValue::new(b"'()'");
//         assert_eq!(var.is_array(), false);
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"\\(\\)"),
//         });
//         assert_eq!(var.text().unwrap(), "()");

//         let var = BashValue::new(b"([0]=first [1]='second item')");
//         assert_eq!(var.is_array(), true);
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"first"),
//             ByteString::new("1") => BashValue::new(b"second\\ item"),
//         });
//         assert_eq!(var.text().unwrap(), "first second item");

//         let var = BashValue::new(b"(nonAssociative)");
//         assert_eq!(var.is_array(), true);
//         assert_eq!(var.array().unwrap(), hash_map! {
//             ByteString::new("nonAssociative") => BashValue::new(b"''"),
//         });
//         assert_eq!(var.text().unwrap(), "nonAssociative");
//     }

//     macro_rules! tmpfile {
//         ($($arg:tt)*) => {{
//             let mut tmp = NamedTempFile::new().expect("could not create temporary file");
//             write!(tmp, $($arg)*).expect("could not write contents to temporary file");
//             tmp.into_temp_path()
//         }};
//     }

//     #[test]
//     fn source_variables() {
//         let tmp = tmpfile! {"
//             simple='just basic text'
//             var=$(echo \"hi\")

//             declare -a some_array
//             some_array[0]=firstItem
//             some_array[1]='second\nItem'
//             some_array[2]=done
//         "};

//         let vars = source(&tmp).unwrap();
//         let var = |name: &str| vars.get(name.as_bytes()).unwrap();

//         assert_eq!(var("simple"), &BashValue::new(b"'just basic text'"));
//         assert_eq!(var("simple").text().unwrap(), "just basic text");

//         assert_eq!(var("var"), &BashValue::new(b"hi"));
//         assert_eq!(var("var").text().unwrap(), "hi");

//         assert_eq!(var("some_array"), &BashValue::new(b"([0]=\"firstItem\" [1]=$'second\\nItem' [2]=\"done\")"));
//         assert_eq!(var("some_array").array().unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"firstItem"),
//             ByteString::new("1") => BashValue::new(b"$'second\\nItem'"),
//             ByteString::new("2") => BashValue::new(b"done"),
//         });
//         assert_eq!(var("some_array").text().unwrap(), "firstItem second\nItem done");
//     }

//     #[test]
//     fn mapfile_string_to_array() {
//         assert_eq!(mapfile(b' ', "\"-S string -T 'multi word text'\"").unwrap(), hash_map! {
//             ByteString::new("0") => BashValue::new(b"-S"),
//             ByteString::new("1") => BashValue::new(b"string"),
//             ByteString::new("2") => BashValue::new(b"-T"),
//             ByteString::new("3") => BashValue::new(b"\\'multi"),
//             ByteString::new("4") => BashValue::new(b"word"),
//             ByteString::new("5") => BashValue::new(b"text\\'"),
//         });
//     }
// }
