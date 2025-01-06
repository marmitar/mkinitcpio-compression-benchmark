//! Handles UNIX user spec in the format `user:group`.

use std::fmt::{self, Write};
use std::str::FromStr;

use anyhow::{Result, bail};
use nix::unistd::{Group, Uid, User};

/// Represents a UNIX user spec from format `user:group`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserSpec {
    /// The user part of `user:group`.
    pub owner: Option<User>,
    /// The group part of `user:group`.
    pub group: Option<Group>,
}

impl UserSpec {
    /// Returns the default user spec for the current user.
    ///
    /// # Example
    ///
    /// ```
    /// # use mkinitcpio_compression_benchmark::UserSpec;
    /// let spec = UserSpec::current_user()?;
    /// println!("{spec}"); // could be "root:root"
    /// # anyhow::Ok(())
    /// ```
    ///
    /// # Errors
    ///
    /// - Runtime UNIX errors (`EINTR`, `ENOMEM`, `ERANGE`, `EMFILE`, etc.)
    pub fn current_user() -> Result<Self> {
        let Some(owner) = User::from_uid(Uid::current())? else {
            bail!("could not find current user (uid = {})", Uid::current());
        };
        let Some(group) = Group::from_gid(owner.gid)? else {
            bail!("could not find login group of user '{}'", owner.name);
        };
        Ok(Self {
            owner: Some(owner),
            group: Some(group),
        })
    }

    /// Parse a UNIX user spec.
    ///
    /// Uses the same format as [`chown(1)`](https://man.archlinux.org/man/chown.1):
    /// - `"user:group"` will set [`st_uid`](nix::sys::stat::FileStat::st_uid) of output files and folder to `user`, and
    ///   [`st_gid`](nix::sys::stat::FileStat::st_gid) to `group`.
    /// - `"user:"` is equivalent to `user:login_group_of_user`, where `login_group_of_user` is [`User::gid`].
    /// - `"user"` (without `:`) will only set [`st_uid`](nix::sys::stat::FileStat::st_uid) of output files, without
    ///     changing [`st_gid`](nix::sys::stat::FileStat::st_gid).
    /// - `":group"` will set [`st_gid`](nix::sys::stat::FileStat::st_gid) only, but not
    ///     [`st_uid`](nix::sys::stat::FileStat::st_uid).
    /// - `":"` and `""` will not change the owner of output files and folders.
    ///
    /// The `user` spec may be a user name or a numeric user ID. For a user ID, whitespace around the number is not
    /// considered. If the `user` spec starts with `+` (e.g. `+0`), then the ID search is performed first, falling back
    /// to user name search. Otherwise, the user name search is done first, and user ID is performed as fallback. The
    /// same applies for the `group` spec.
    ///
    /// # Errors
    ///
    /// - Invalid spec string.
    /// - Specified user could not be found.
    /// - Specified group could not be found.
    /// - Runtime UNIX errors (`EINTR`, `ENOMEM`, `ERANGE`, `EMFILE`, etc.)
    #[inline]
    pub fn from_spec(spec: impl AsRef<str>) -> Result<Self> {
        spec.as_ref().parse()
    }

    /// Formats [`UserSpec`] as `+uid:+gid`.
    ///
    /// The string returned will be parsed correctly with [`UserSpec::from_spec`], as long the system keeps user and
    /// groups information valid (i.e. the group was not deleted, or the user didn't change ID).
    ///
    /// # Example
    ///
    /// ```
    /// # use mkinitcpio_compression_benchmark::UserSpec;
    /// let spec = UserSpec::from_spec("root:")?;
    /// assert_eq!(spec.to_string(), "root:root");
    /// assert_eq!(format!("{:+}", spec.to_numeric_spec()), "+0:+0");
    /// assert_eq!(UserSpec::from_spec(spec.to_spec().to_string())?, spec);
    /// # anyhow::Ok(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn to_numeric_spec(&self) -> impl Copy + fmt::Display + '_ {
        UserSpecFormatter {
            spec: self,
            numeric: true,
        }
    }

    /// Formats [`UserSpec`] as `username:groupname`.
    ///
    /// The string returned will be parsed correctly with [`UserSpec::from_spec`], as long the system keeps user and
    /// groups information valid (i.e. the group was not deleted, or the user didn't change ID).
    ///
    /// # Example
    ///
    /// ```
    /// # use mkinitcpio_compression_benchmark::UserSpec;
    /// let spec = UserSpec::from_spec("root:")?;
    /// assert_eq!(spec.to_spec().to_string(), "root:root");
    /// assert_eq!(spec.to_string(), "root:root");
    /// assert_eq!(UserSpec::from_spec(spec.to_spec().to_string())?, spec);
    /// # anyhow::Ok(())
    /// ```
    #[inline]
    #[must_use]
    pub const fn to_spec(&self) -> impl Copy + fmt::Display + '_ {
        UserSpecFormatter {
            spec: self,
            numeric: false,
        }
    }
}

/// Formats [`UserSpec`] as `username:groupname`.
impl fmt::Display for UserSpec {
    /// Formats [`UserSpec`] as `username:groupname`.
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.owner {
            f.write_str(&user.name)?;
        }
        if let Some(group) = &self.group {
            f.write_char(':')?;
            f.write_str(&group.name)?;
        }
        Ok(())
    }
}

/// See [`UserSpec::from_spec`].
impl FromStr for UserSpec {
    type Err = anyhow::Error;

    /// See [`UserSpec::from_spec`].
    #[inline]
    fn from_str(spec: &str) -> anyhow::Result<Self> {
        parse_spec(spec)
    }
}

/// Handles customized formatting for [`UserSpec`].
#[derive(Clone, Copy)]
struct UserSpecFormatter<'a> {
    /// [`UseSpec`] to be formatted.
    spec: &'a UserSpec,
    /// If output should be `+uid:+gid` or `username:groupname`.
    numeric: bool,
}

impl fmt::Debug for UserSpecFormatter<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.spec, f)
    }
}

impl fmt::Display for UserSpecFormatter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(user) = &self.spec.owner {
            if self.numeric {
                fmt::Display::fmt(&user.uid, f)?;
            } else {
                fmt::Display::fmt(&user.name, f)?;
            }
        }
        if let Some(group) = &self.spec.group {
            f.write_char(':')?;

            if self.numeric {
                fmt::Display::fmt(&group.gid, f)?;
            } else {
                fmt::Display::fmt(&group.name, f)?;
            }
        }
        Ok(())
    }
}

/// See [`UserSpec::from_spec`].
fn parse_spec(spec: &str) -> Result<UserSpec> {
    let (username, groupname, has_colon) = match spec.split_once(':') {
        Some((user, group)) => (user, group, true),
        None => (spec, "", false),
    };

    let user = get_item("user", username, User::from_name, User::from_uid)?;
    let mut group = get_item("group", groupname, Group::from_name, Group::from_gid)?;

    // A separator was given, but a group was not specified, so get the login group.
    if group.is_none() && has_colon {
        if let Some(user) = &user {
            let Some(login_group) = Group::from_gid(user.gid)? else {
                bail!("invalid login group {} for user '{}'", user.gid, user.name);
            };
            group = Some(login_group);
        }
    }

    Ok(UserSpec { owner: user, group })
}

/// Parse either a user or a group from `user:group` spec.
///
/// This handles both name and ID search. If the spec starts with `+`, then
/// ID search is performed first, and assumed to be the default, otherwise
/// name search is performed first. If the first search fails, the other
/// method is tried.
fn get_item<T, U: From<u32>>(
    item_desc: &str,
    spec: &str,
    by_name: impl FnOnce(&str) -> nix::Result<Option<T>>,
    by_id: impl FnOnce(U) -> nix::Result<Option<T>>,
) -> Result<Option<T>> {
    let parse_name = |name: &str| {
        if name.is_empty() {
            return Ok(None);
        }
        let Some(item) = by_name(name)? else {
            bail!("could not find {item_desc} with name: '{name}'")
        };
        Ok(Some(item))
    };

    let parse_id = |id: &str| {
        if id.is_empty() {
            return Ok(None);
        }
        let Some(item) = by_id(u32::from_str(id)?.into())? else {
            bail!("could not find {item_desc} with id: {id}");
        };
        Ok(Some(item))
    };

    let trimmed = spec.trim();
    if trimmed.starts_with('+') || trimmed.starts_with('-') {
        // +NUM defaults to userid, but fallbacks to username
        parse_id(trimmed).or_else(|original_error| parse_name(spec).ok().ok_or(original_error))
    } else {
        // otherwise defaults to username, but fallbacks to userid
        parse_name(spec).or_else(|original_error| parse_id(trimmed).ok().ok_or(original_error))
    }
}

#[cfg(test)]
mod parsing {
    use nix::unistd::ROOT;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn conforms_to_chown_username() {
        let root = User::from_uid(ROOT).unwrap().unwrap();
        let groot = Group::from_gid(root.gid).unwrap().unwrap();
        let log = Group::from_name("log").unwrap().unwrap();

        let spec = UserSpec::from_spec("root:log").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec("root:").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&groot), "group is root");

        let spec = UserSpec::from_spec(":log").unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec("root").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");

        let spec = UserSpec::from_spec("").unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");

        let spec = UserSpec::from_spec(":").unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");
    }

    #[test]
    fn conforms_to_chown_uid() {
        let root = User::from_uid(ROOT).unwrap().unwrap();
        let groot = Group::from_gid(root.gid).unwrap().unwrap();
        let log = Group::from_name("log").unwrap().unwrap();

        let spec = UserSpec::from_spec(format!("0:{}", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec(format!("+0:+{}", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec("0:").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&groot), "group is root");

        let spec = UserSpec::from_spec("+0:").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&groot), "group is root");

        let spec = UserSpec::from_spec(format!(":{}", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec(format!(":+{}", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec("0").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");

        let spec = UserSpec::from_spec("+0").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");
    }

    #[test]
    fn ignores_whitespace_for_numeric_id() {
        let root = User::from_uid(ROOT).unwrap().unwrap();
        let groot = Group::from_gid(root.gid).unwrap().unwrap();
        let log = Group::from_name("log").unwrap().unwrap();

        let spec = UserSpec::from_spec(format!("  0 :   {}  ", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec(" 0 : ").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), Some(&groot), "group is root");

        let spec = UserSpec::from_spec(format!(" : {} ", log.gid)).unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), Some(&log), "group is log");

        let spec = UserSpec::from_spec(" 0 ").unwrap();
        assert_eq!(spec.owner.as_ref(), Some(&root), "owner is root");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");

        let spec = UserSpec::from_spec("   ").unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");

        let spec = UserSpec::from_spec("  :   ").unwrap();
        assert_eq!(spec.owner.as_ref(), None, "owner unspecified");
        assert_eq!(spec.group.as_ref(), None, "group unspecified");
    }

    #[test]
    fn doesnt_ignore_whitespace_for_username() {
        let error = UserSpec::from_spec(" root : log ").unwrap_err();
        assert_eq!(error.to_string(), "could not find user with name: ' root '");

        let error = UserSpec::from_spec("root  :  ").unwrap_err();
        assert_eq!(error.to_string(), "could not find user with name: 'root  '");

        let error = UserSpec::from_spec(" : log  ").unwrap_err();
        assert_eq!(error.to_string(), "could not find group with name: ' log  '");

        let error = UserSpec::from_spec(" root").unwrap_err();
        assert_eq!(error.to_string(), "could not find user with name: ' root'");
    }
}

#[cfg(test)]
mod display {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn to_spec_returns_parseable_string() {
        let spec = UserSpec::current_user().unwrap();
        let owner = spec.owner.as_ref().unwrap();
        let group = spec.group.as_ref().unwrap();

        assert_eq!(format!("{:+}", spec.to_numeric_spec()), format!("+{}:+{}", owner.uid, group.gid));
        assert_eq!(UserSpec::from_spec(spec.to_numeric_spec().to_string()).unwrap(), spec);

        assert_eq!(spec.to_string(), format!("{}:{}", owner.name, group.name));
        assert_eq!(UserSpec::from_spec(spec.to_string()).unwrap(), spec);

        assert_eq!(UserSpec::from_spec(format!(" {}  :  {} ", owner.uid, group.gid)).unwrap(), spec);
    }
}
