//! Turning a TYPED path argument into a real path (v0.138.2).
//!
//! Every command that accepts a path from the search bar — `disk`, `loc`,
//! `repo`, `md2pdf` — used to do `PathBuf::from(arg)`, which takes a leading
//! `~` literally: a *relative* folder named `~`, which never exists. So
//! `daisy ~/Downloads` silently scanned nothing, while the command's own help
//! gave exactly that as its example. One shared helper now, so the next
//! path-taking command inherits the behaviour instead of the bug.

use std::path::{Path, PathBuf};

/// Resolve a typed path: a leading `~` (alone or `~/…`) becomes the home
/// folder, surrounding quotes are dropped.
///
/// Quotes are handled because of how paths reach the search bar — dragged in
/// from a terminal, or pasted from one, they arrive as `"/Users/x/My Files"`.
///
/// A `~user` form is deliberately NOT expanded: resolving another account's
/// home needs the password database, and guessing `/Users/<name>` would be
/// wrong on any machine that doesn't follow that layout. Leaving it literal
/// fails visibly ("no such folder") instead of scanning the wrong one.
pub fn expand_user(arg: &str, home: Option<&Path>) -> PathBuf {
    let mut s = arg.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            s = &s[1..s.len() - 1];
            break;
        }
    }
    let s = s.trim();
    match (s, home) {
        ("~", Some(h)) => h.to_path_buf(),
        (_, Some(h)) if s.starts_with("~/") => h.join(&s[2..]),
        _ => PathBuf::from(s),
    }
}

/// The same, for a caller that hands its paths on as strings (`loc` feeds
/// tokei a `&[String]`).
pub fn expand_user_str(arg: &str, home: Option<&Path>) -> String {
    expand_user(arg, home).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_leading_tilde() {
        let home = PathBuf::from("/Users/t");
        assert_eq!(expand_user("~/Downloads", Some(&home)), PathBuf::from("/Users/t/Downloads"));
        assert_eq!(expand_user("~", Some(&home)), home);
        assert_eq!(expand_user("  ~/a/b  ", Some(&home)), PathBuf::from("/Users/t/a/b"));
    }

    #[test]
    fn leaves_everything_else_alone() {
        let home = PathBuf::from("/Users/t");
        assert_eq!(expand_user("/tmp", Some(&home)), PathBuf::from("/tmp"));
        assert_eq!(expand_user("relative/dir", Some(&home)), PathBuf::from("relative/dir"));
        // `~user` needs the password database — guessing /Users/<name> would be
        // wrong on machines that don't use that layout, so it stays literal.
        assert_eq!(expand_user("~bob/x", Some(&home)), PathBuf::from("~bob/x"));
        // A tilde INSIDE the path is an ordinary character.
        assert_eq!(expand_user("/tmp/~x", Some(&home)), PathBuf::from("/tmp/~x"));
        // No home known → never silently resolve to the wrong place.
        assert_eq!(expand_user("~/Downloads", None), PathBuf::from("~/Downloads"));
    }

    #[test]
    fn strips_quotes_a_dragged_in_path_carries() {
        let home = PathBuf::from("/Users/t");
        assert_eq!(expand_user("\"/tmp/My Files\"", Some(&home)), PathBuf::from("/tmp/My Files"));
        assert_eq!(expand_user("'~/Downloads'", Some(&home)), PathBuf::from("/Users/t/Downloads"));
        // Only a MATCHED surrounding pair — an apostrophe in a folder name is
        // part of the name.
        assert_eq!(expand_user("/tmp/martin's", Some(&home)), PathBuf::from("/tmp/martin's"));
    }

    #[test]
    fn a_git_url_is_never_touched() {
        // `repo` routes URLs down a different branch, but the helper must be
        // safe either way — no leading `~`, so nothing happens.
        let home = PathBuf::from("/Users/t");
        for url in [
            "https://github.com/pepperonas/inspector-rust.git",
            "git@github.com:pepperonas/inspector-rust.git",
        ] {
            assert_eq!(expand_user(url, Some(&home)), PathBuf::from(url));
        }
    }

    #[test]
    fn the_string_form_agrees_with_the_path_form() {
        let home = PathBuf::from("/Users/t");
        for arg in ["~/Downloads", "/tmp", "'~/x'", "rel"] {
            assert_eq!(
                expand_user_str(arg, Some(&home)),
                expand_user(arg, Some(&home)).to_string_lossy()
            );
        }
    }
}
