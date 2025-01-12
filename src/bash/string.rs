//! Working with Bash strings.

use std::borrow::Cow;
use std::borrow::{Borrow, BorrowMut};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::str::{FromStr, Utf8Error};

use anyhow::{Context, Result};
use format_bytes::{DisplayBytes, format_bytes};
use tempfile::NamedTempFile;

use super::BashArray;
use super::exec;

/// Apply Bash quoting rules for binary data.
///
/// - Doesn't escape simple strings.
/// - Usually apply single quotes for spaces and parenthesis.
/// - Uses `$'...'` evalutation for escaped characters (e.g. `\n`).
/// - Doesn't work nicely with `\0`
fn escape(data: &[u8]) -> Result<Box<str>> {
    log::trace!("escape: input={}", data.escape_ascii());
    let mut temp = NamedTempFile::new()?;
    temp.write_all(data)?;
    let temp = temp.into_temp_path();
    log::trace!("escape: temp file={}", temp.display());

    let (dir, file) = exec::resolve_file(&temp)?;
    let command = format_bytes!(b"OUTPUT=\"$(cat '{}')\"", file.as_bytes());

    let output = exec::rbash_with_output_at(&command, &dir)?.into();
    log::trace!("escape: output={output:?}");
    Ok(output)
}

/// Resolve a quoted Bash string.
///
/// Works mostly as the inverse of [`escape`].
fn unescape(text: &str) -> Result<Box<[u8]>> {
    log::trace!("escape: input={text:?}");
    let command = format_bytes!(
        b"INPUT={}
        echo -n \"$INPUT\"",
        text.trim().as_bytes(),
    );

    let output = exec::rbash(&command)?.into();
    log::trace!("unescape: output={output:?}");
    Ok(output)
}

/// Represents a normal string value in Bash, quoted and unquoted.
#[derive(Clone, Eq)]
pub struct BashString {
    /// Quoted version of the string.
    escaped: Box<str>,
    /// Unquoted version of the string.
    raw: Box<[u8]>,
}

impl BashString {
    /// See [`Self::from_escaped`].
    fn from_escaped_boxed(escaped: Box<str>) -> Result<Self> {
        let raw = unescape(&escaped).with_context(|| format!("while parsing possibly escaped text: {escaped:?}"))?;
        Ok(Self { escaped, raw })
    }

    /// Resolves a Bash string from its quoted form.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for invalid or unquoted data, and for other runtime errors in Bash.
    pub fn from_escaped(text: impl Into<Box<str>>) -> Result<Self> {
        Self::from_escaped_boxed(text.into())
    }

    /// See [`Self::from_raw`].
    fn from_raw_boxed(raw: Box<[u8]>) -> Result<Self> {
        let escaped = escape(&raw).with_context(|| format!("while escaping raw bytes: {}", repr_byte_str(&raw)))?;
        Ok(Self { escaped, raw })
    }

    /// Resolves a Bash string from its unquoted form.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    #[inline]
    pub fn from_raw(bytes: impl Into<Box<[u8]>>) -> Result<Self> {
        Self::from_raw_boxed(bytes.into())
    }

    /// Quoted form of the string.
    #[inline]
    #[must_use]
    pub const fn source(&self) -> &str {
        &self.escaped
    }

    /// Unquoted form of the string.
    #[inline]
    #[must_use]
    pub const fn as_raw(&self) -> &[u8] {
        &self.raw
    }

    /// Unquoted string as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] if the raw string is not UTF-8 data.
    #[inline]
    pub const fn as_utf8(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.as_raw())
    }

    /// Unquoted string as UTF-8, replacing invalid characters.
    #[must_use]
    pub fn to_utf8_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_raw())
    }

    /// Unquoted string as valid Rust source.
    #[inline]
    #[must_use]
    pub fn as_repr(&self) -> String {
        repr_byte_str(self.as_raw())
    }

    /// Convert bytes to a path.
    #[inline]
    #[must_use]
    pub fn as_path(&self) -> &Path {
        Path::new(OsStr::from_bytes(self.as_raw()))
    }

    /// Uses `read` to split the string into an array.
    ///
    /// Splitting is done at spaces, with `IFS=' '`.
    ///
    /// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    pub fn arrayize(&self) -> Result<BashArray> {
        let command = format_bytes!(
            b"set -f
            INPUT={}
            read -r -a OUTPUT <<< \"$INPUT\"",
            self.escaped.as_bytes(),
        );

        let output = exec::rbash_with_output(&command)?;
        BashArray::new(output)
    }

    /// Uses `mapfile` to split the string into an array.
    ///
    /// Splitting is done according to `delimiter`.
    ///
    /// For more details, see [bash(1)](https://man.archlinux.org/man/bash.1).
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    pub fn mapfile(&self, delimiter: u8) -> Result<BashArray> {
        let command = format_bytes!(
            b"declare -a OUTPUT=()
            INPUT={}
            mapfile -d '{}' -t OUTPUT 1>&- < <(
                printf '%s' \"$INPUT\"
            )",
            self.escaped.as_bytes(),
            [delimiter],
        );

        let output = exec::rbash_with_output(&command)?;
        BashArray::new(output)
    }

    /// Recreate a quoted string from it's byte contents.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] for runtime errors in Bash.
    pub fn reescape(&self) -> Result<Self> {
        Self::from_raw(self.as_raw())
    }
}

/// Tries to convert as a quoted string, but uses unquoted string as fallback.
///
/// See [`BashString::from_escaped`] and [`BashString::from_raw`].
impl FromStr for BashString {
    type Err = anyhow::Error;

    /// Tries to convert as a quoted string, but uses unquoted string as fallback.
    ///
    /// See [`BashString::from_escaped`] and [`BashString::from_raw`].
    fn from_str(text: &str) -> Result<Self> {
        Self::from_escaped(text).or_else(|_| Self::from_raw(text.as_bytes()))
    }
}

/// Represents a single byte with hexadecimal escape (`\x`).
const fn repr_byte(byte: u8) -> [u8; 4] {
    const fn hex(num: u8) -> u8 {
        match num {
            0..=9 => b'0' + num,
            10..=16 => b'A' + (num - 10),
            _ => panic!("number not in hexadecimal range"),
        }
    }

    #[expect(clippy::integer_division_remainder_used, reason = "optmized by the compiler")]
    [b'\\', b'x', hex(byte / 16), hex(byte % 16)]
}

/// Represents a byte array as Rust source, escaping non UTF-8 characters with hexadecimal (`\x`).
fn repr_byte_str(bytes: &[u8]) -> String {
    let content = bytes.utf8_chunks().flat_map(|chunk| {
        let valid = chunk.valid().escape_debug();
        let invalid = chunk.invalid().iter().copied().flat_map(repr_byte).map(Into::into);
        valid.chain(invalid)
    });

    let mut out = String::with_capacity(bytes.len());
    out.push_str("b\"");
    out.extend(content);
    out.push('"');
    out
}

impl fmt::Debug for BashString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.source())
    }
}

impl fmt::Display for BashString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.as_raw().utf8_chunks() {
            fmt::Display::fmt(chunk.valid(), f)?;
            if !chunk.invalid().is_empty() {
                fmt::Display::fmt(&char::REPLACEMENT_CHARACTER, f)?;
            }
        }
        Ok(())
    }
}

impl DisplayBytes for BashString {
    #[inline]
    fn display_bytes(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(self.as_raw())
    }
}

impl<T: AsRef<[u8]> + ?Sized> PartialEq<T> for BashString {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        self.as_raw().eq(other.as_ref())
    }
}

impl<T: AsRef<[u8]> + ?Sized> PartialOrd<T> for BashString {
    #[inline]
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        self.as_raw().partial_cmp(other.as_ref())
    }
}

impl Ord for BashString {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_raw().cmp(other.as_raw())
    }
}

impl Hash for BashString {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_raw().hash(state);
    }
}

impl<T: ?Sized> AsRef<T> for BashString
where
    Box<[u8]>: AsRef<T>,
{
    #[inline]
    fn as_ref(&self) -> &T {
        self.raw.as_ref()
    }
}

impl<T: ?Sized> AsMut<T> for BashString
where
    Box<[u8]>: AsMut<T>,
{
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.raw.as_mut()
    }
}

impl<T: ?Sized> Borrow<T> for BashString
where
    Box<[u8]>: Borrow<T>,
{
    #[inline]
    fn borrow(&self) -> &T {
        self.raw.borrow()
    }
}

impl<T: ?Sized> BorrowMut<T> for BashString
where
    Box<[u8]>: BorrowMut<T>,
{
    #[inline]
    fn borrow_mut(&mut self) -> &mut T {
        self.raw.borrow_mut()
    }
}

impl Deref for BashString {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.raw
    }
}

impl DerefMut for BashString {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.raw
    }
}

#[cfg(test)]
mod conversion {
    use pretty_assertions::{assert_eq, assert_matches};
    use test_log::test;

    use super::*;

    #[test]
    fn escaping() {
        let raw = b"just some text";
        let escaped = escape(raw).unwrap();
        assert_eq!(&*escaped, "'just some text'");
        assert_eq!(&*unescape(&escaped).unwrap(), raw);

        let raw = b"binary\xFF\xFFdata";
        let escaped = escape(raw).unwrap();
        assert_eq!(&*escaped, "$'binary\\377\\377data'");
        assert_eq!(&*unescape(&escaped).unwrap(), raw);
    }

    #[test]
    fn non_escaped_text() {
        assert_eq!(&*unescape("word").unwrap(), b"word");

        let err = unescape("multiple words").unwrap_err();
        assert_matches!(err.to_string(), s if s.contains("words: command not found"));
    }

    #[test]
    fn rust_representation() {
        assert_eq!(repr_byte_str(b"text with \0 binary \xFF data"), stringify!(b"text with \0 binary \xFF data"));
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
    use pretty_assertions::{assert_eq, assert_ne};
    use test_log::test;

    use super::*;

    #[test]
    fn partial_eq() {
        let string = BashString::from_raw(b"Hello \xF0\x90\x80World".as_slice()).unwrap();
        assert_eq!(string.to_utf8_lossy(), "Hello �World");
        assert_ne!(string, "Hello �World");
        assert_eq!(string, string.reescape().unwrap());

        let string = BashString::from_escaped("'Simple string!'").unwrap();
        assert_eq!(string.as_utf8(), Ok("Simple string!"));
        assert_eq!(string.to_utf8_lossy(), "Simple string!");
        assert_eq!(string, "Simple string!");
        assert_eq!(string, string.reescape().unwrap());
    }

    #[test]
    fn deref() {
        let data = b"has invalid UTF8: \xF0\x90\x80 !!!".as_slice();
        let string = BashString::from_raw(data).unwrap();
        assert_eq!(string.trim_ascii(), data);
    }

    #[test]
    fn from_str() {
        assert_eq!(BashString::from_str("'escaped text'").unwrap(), "escaped text");
        assert_eq!(BashString::from_str("just normal text").unwrap(), "just normal text");
    }

    #[test]
    fn diplay_debug_fmt() {
        let string = BashString::from_raw(b"Hello \xF0\x90\x80World".as_slice()).unwrap();
        assert_eq!(string.as_repr(), stringify!(b"Hello \xF0\x90\x80World"));
        assert_eq!(format!("{:?}", string), "$'Hello \\360\\220\\200World'");
        assert_eq!(string.to_string(), "Hello �World");

        let text = "another string";
        let string = BashString::from_raw(text.as_bytes()).unwrap();
        assert_eq!(format!("{string:?}"), format!("'{text}'"));
        assert_eq!(string.to_string(), text);
    }
}
