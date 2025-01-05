//! Working with Bash arrays.

use core::fmt::{self, Write};
use core::hash::{Hash, Hasher};
use core::iter::FusedIterator;

use anyhow::{Result, bail};
use format_bytes::format_bytes;

use super::BashString;
use super::exec;

/// Test if a quoted string represents an array in Bash.
///
/// This just checks if the string is enclosed in parenthesis, without doing a full parsing. For escaped strings (e.g.
/// from `declare` or `printf '%q'` output) checking only the first byte should be enough.
pub(super) const fn is_array_source(text: &str) -> bool {
    matches!(text.as_bytes().first().copied(), Some(b'(')) && matches!(text.as_bytes().last().copied(), Some(b')'))
}

/// Parses an escaped Bash array into a list of `(index, string)` pairs.
fn parse_array_content(text: &str) -> Result<Box<[(i32, BashString)]>> {
    if !is_array_source(text) {
        bail!("invalid array source: {text}");
    }

    let command = format_bytes!(
        b"declare -a ARR={}
        for KEY in \"${}!ARR[@]{}\"; do
            printf '%q=%q\\n' \"$KEY\" \"${}ARR[$KEY]{}\"
        done",
        text.as_bytes(),
        b"{",
        b"}",
        b"{",
        b"}",
    );

    let output = exec::rbash(&command)?;
    super::parse_vars(output, |key| Ok(key.parse()?), |value| BashString::from_escaped(value))
}

/// Represents an indexed arrayin Bash.
#[derive(Clone, PartialEq, Eq)]
pub struct BashArray {
    /// Quoted version of the array.
    source: Box<str>,
    /// Parsed list of `(index, string)` pairs.
    content: Box<[(i32, BashString)]>,
}

impl BashArray {
    /// See [`Self::new`].
    fn new_from_boxed(source: Box<str>) -> Result<Self> {
        let content = parse_array_content(source.trim())?;
        Ok(Self { source, content })
    }

    /// Parse a Bash array from a quoted string source.
    ///
    /// This usually expects an output from `declare`, which should be on the form `([idx]="text" ...)`, but should also
    /// work with simple `(text ...)` arrays.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for invalid or unquoted data, and for other runtime errors in Bash.
    #[inline]
    pub fn new(source: impl Into<Box<str>>) -> Result<Self> {
        Self::new_from_boxed(source.into())
    }

    /// Quoted form of the string.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &str {
        &self.source
    }

    /// Converts the array to a Bash string.
    ///
    /// Equivalent to `$ARRAY`, so usually only the first element is converted.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    #[inline]
    pub fn to_bash_string(&self) -> Result<BashString> {
        BashString::from_escaped(self.source.as_ref())
    }

    /// Concatenes the array into a single Bash string.
    ///
    /// Equivalent to `${ARRAY[*]}`.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    pub fn to_concatenated_string(&self) -> Result<BashString> {
        let command = format_bytes!(
            b"declare -a ARRAY={}
            OUTPUT=\"${}ARRAY[*]{}\"",
            self.source.as_bytes(),
            b"{",
            b"}"
        );

        let output = exec::rbash_with_output(&command)?;
        BashString::from_escaped(output)
    }

    /// Iterator of the `(index, string)` pairs.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = (i32, &BashString)> + ExactSizeIterator + FusedIterator {
        self.content.iter().map(|(key, val)| (*key, val))
    }

    /// Iterator of the indexes of the array.
    #[inline]
    #[must_use]
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = i32> + ExactSizeIterator + FusedIterator {
        self.entries().map(|(key, _)| key)
    }

    /// Iterator of the string values in the array.
    #[inline]
    #[must_use]
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &BashString> + ExactSizeIterator + FusedIterator {
        self.entries().map(|(_, value)| value)
    }

    /// Consuming iterator of the `(index, string)` pairs.
    #[inline]
    #[must_use]
    pub fn into_entries(
        self,
    ) -> impl DoubleEndedIterator<Item = (i32, BashString)> + ExactSizeIterator + FusedIterator {
        self.content.into_vec().into_iter()
    }

    /// Consuming iterator of the string values in the array.
    #[inline]
    #[must_use]
    pub fn into_values(self) -> impl DoubleEndedIterator<Item = BashString> + ExactSizeIterator + FusedIterator {
        self.into_entries().map(|(_, value)| value)
    }
}

impl<T: AsRef<[u8]> + ?Sized, I: ?Sized> PartialEq<I> for BashArray
where
    for<'a> &'a I: IntoIterator<Item = &'a T>,
{
    fn eq(&self, other: &I) -> bool {
        let mut this = self.values();
        let mut that = other.into_iter();
        loop {
            match (this.next(), that.next()) {
                (Some(a), Some(b)) if a == b => continue,
                (None, None) => return true,
                (_, _) => return false,
            }
        }
    }
}

impl fmt::Debug for BashArray {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl fmt::Display for BashArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('(')?;
        for (idx, string) in self.values().enumerate() {
            if idx > 0 {
                f.write_char(' ')?;
            }
            f.write_str(string.source())?;
        }
        f.write_char(')')?;
        Ok(())
    }
}

impl Hash for BashArray {
    fn hash<H: Hasher>(&self, state: &mut H) {
        b'('.hash(state);
        for (idx, (key, value)) in self.entries().enumerate() {
            if idx > 0 {
                b' '.hash(state);
            }
            key.hash(state);
            b'='.hash(state);
            value.hash(state);
        }
        b')'.hash(state);
    }
}

#[cfg(test)]
mod conversion {
    use pretty_assertions::{assert_eq, assert_matches};

    use super::*;

    #[test]
    fn parsing() {
        let array = BashArray::new("([0]=standard [1]=array)").unwrap();
        assert_eq!(array.source(), "([0]=standard [1]=array)");
        assert_eq!(array, ["standard", "array"]);

        let array = BashArray::new("(simplified 'bash array')").unwrap();
        assert_eq!(array.source(), "(simplified 'bash array')");
        assert_eq!(array, ["simplified", "bash array"]);

        let array = BashArray::new("([0]='bash\narray' [10]=with [100]=holes)").unwrap();
        assert_eq!(array.source(), "([0]='bash\narray' [10]=with [100]=holes)");
        assert_eq!(array, ["bash\narray", "with", "holes"]);

        let array = BashArray::new("($(echo some output))").unwrap();
        assert_eq!(array.source(), "($(echo some output))");
        assert_eq!(array, ["some", "output"]);
    }

    #[test]
    fn non_escaped_text() {
        let err = BashArray::new("just text").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("invalid array source"));

        let err = BashArray::new("(not closed").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("invalid array source"));

        let err = BashArray::new("(unclosed quote')").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("unexpected EOF"));
    }

    #[test]
    fn stringfication() {
        let array = BashArray::new("(simplified array)").unwrap();
        assert_eq!(array, ["simplified", "array"]);
        assert_eq!(array.to_bash_string().unwrap(), "simplified");
        assert_eq!(array.to_concatenated_string().unwrap(), "simplified array");

        let array = BashArray::new("([0]='single entry' [1]=another)").unwrap();
        assert_eq!(array, ["single entry", "another"]);
        assert_eq!(array.to_bash_string().unwrap(), "single entry");
        assert_eq!(array.to_concatenated_string().unwrap(), "single entry another");

        let array = BashArray::new("([0]='bash\narray' [10]=with [100]=holes)").unwrap();
        assert_eq!(array, ["bash\narray", "with", "holes"]);
        assert_eq!(array.to_bash_string().unwrap(), "bash\narray");
        assert_eq!(array.to_concatenated_string().unwrap(), "bash\narray with holes");
    }

    #[test]
    fn arrayize() {
        let string = BashString::from_raw(*b"string 'with quotes' and\0 null byte").unwrap();
        assert_eq!(string.source(), "'string '\\''with quotes'\\'' and null byte'");

        assert_eq!(string.arrayize().unwrap(), ["string", "'with", "quotes'", "and", "null", "byte"]);
        assert_eq!(string.mapfile(b' ').unwrap(), ["string", "'with", "quotes'", "and", "null", "byte"]);
        assert_eq!(string.mapfile(b'\0').unwrap(), ["string 'with quotes' and null byte"]);
    }
}

#[cfg(test)]
mod basic_impl {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn partial_eq() {
        let array = BashArray::new("(a b c)").unwrap();
        assert_eq!(array.source(), "(a b c)");
        assert_eq!(array, ["a", "b", "c"]);

        let array = BashArray::new("([0]=x [10]=y [12]='z z')").unwrap();
        assert_eq!(array, ["x", "y", "z z"]);
    }

    #[test]
    fn diplay_debug_fmt() {
        let array = BashArray::new("([0]=first [3]='second item')").unwrap();
        assert_eq!(format!("{array}"), "(first second\\ item)");
        assert_eq!(format!("{array:?}"), "([0]=first [3]='second item')");

        let array = BashArray::new("('\n')").unwrap();
        assert_eq!(format!("{array}"), "($'\\n')");
        assert_eq!(format!("{array:?}"), "('\n')");
    }
}
