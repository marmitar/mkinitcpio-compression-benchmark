use core::fmt::{self, Write};
use core::hash::{Hash, Hasher};
use core::iter::FusedIterator;

use anyhow::{Result, bail};
use format_bytes::format_bytes;

use super::BashString;
use super::exec;

pub(super) const fn is_array_source(text: &str) -> bool {
    matches!(text.as_bytes().first().copied(), Some(b'(')) && matches!(text.as_bytes().last().copied(), Some(b')'))
}

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

#[derive(Clone, PartialEq, Eq)]
pub struct BashArray {
    source: Box<str>,
    content: Box<[(i32, BashString)]>,
}

impl BashArray {
    fn new_from_boxed(source: Box<str>) -> Result<Self> {
        let content = parse_array_content(source.trim())?;
        Ok(Self { source, content })
    }

    #[inline]
    pub fn new(source: impl Into<Box<str>>) -> Result<Self> {
        Self::new_from_boxed(source.into())
    }

    #[inline]
    #[must_use]
    pub const fn source(&self) -> &str {
        &self.source
    }

    #[inline]
    pub fn to_bash_string(&self) -> Result<BashString> {
        BashString::from_escaped(self.source.as_ref())
    }

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

    #[inline]
    #[must_use]
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = (i32, &BashString)> + ExactSizeIterator + FusedIterator {
        self.content.iter().map(|(key, val)| (*key, val))
    }

    #[inline]
    #[must_use]
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = i32> + ExactSizeIterator + FusedIterator {
        self.entries().map(|(key, _)| key)
    }

    #[inline]
    #[must_use]
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &BashString> + ExactSizeIterator + FusedIterator {
        self.entries().map(|(_, value)| value)
    }

    #[inline]
    #[must_use]
    pub fn into_entries(
        self,
    ) -> impl DoubleEndedIterator<Item = (i32, BashString)> + ExactSizeIterator + FusedIterator {
        self.content.into_vec().into_iter()
    }

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
        for string in self.values() {
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
