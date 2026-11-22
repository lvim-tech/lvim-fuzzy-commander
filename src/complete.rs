//! Tab completion for the command line: a command name (from `$PATH`) for the first word of a command, a path
//! (relative to the panel's directory, `~/…` too) for any other word.

use std::path::{Path, PathBuf};

/// Characters a shell needs escaped in a word.
const SPECIAL: &str = " \t'\"\\$&;|<>()*?[]#!`~";

fn escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if SPECIAL.contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn unescape(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(n) = it.next() {
                o.push(n);
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// The word under the end of `before` and whether it starts a command: (byte index where it starts, is a command).
fn word(before: &str) -> (usize, bool) {
    let b = before.as_bytes();
    let mut i = b.len();
    while i > 0 {
        let c = b[i - 1];
        let escaped = i >= 2 && b[i - 2] == b'\\';
        if (c == b' ' || c == b'\t' || c == b';' || c == b'|' || c == b'&' || c == b'(') && !escaped {
            break;
        }
        i -= 1;
    }
    // a command when nothing but separators stands before it
    let prev = before[..i].trim_end();
    let is_cmd = prev.is_empty() || prev.ends_with([';', '|', '&', '(']);
    (i, is_cmd)
}

fn executables(prefix: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for dir in std::env::var("PATH").unwrap_or_default().split(':').filter(|d| !d.is_empty()) {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix) {
                v.push(name);
            }
        }
    }
    v.sort();
    v.dedup();
    v.truncate(500);
    v.iter().map(|n| escape(n)).collect()
}

fn paths(typed: &str, cwd: &Path) -> Vec<String> {
    let raw = unescape(typed);
    let (dir_typed, base) = match raw.rfind('/') {
        Some(i) => (&raw[..=i], &raw[i + 1..]),
        None => ("", raw.as_str()),
    };
    let dir: PathBuf = if let Some(rest) = dir_typed.strip_prefix("~/") {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest)
    } else if dir_typed.starts_with('/') {
        PathBuf::from(dir_typed)
    } else {
        cwd.join(dir_typed)
    };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with(base) || (name.starts_with('.') && !base.starts_with('.')) {
                return None;
            }
            let is_dir = e.path().is_dir();
            Some(format!("{}{}{}", escape(dir_typed).replace("\\~", "~"), escape(&name), if is_dir { "/" } else { "" }))
        })
        .collect();
    v.sort();
    v.truncate(500);
    v
}

/// The completions of the word at the end of `before`: where that word starts (a byte index) and what can replace
/// it (escaped for a shell, a directory ending in `/`).
pub fn candidates(before: &str, cwd: &Path) -> (usize, Vec<String>) {
    let (start, is_cmd) = word(before);
    let w = &before[start..];
    let c = if is_cmd && !w.contains('/') && !w.starts_with(['~', '.']) { executables(&unescape(w)) } else { paths(w, cwd) };
    (start, c)
}

/// The longest start all of `v` share.
pub fn common_prefix(v: &[String]) -> String {
    let Some(first) = v.first() else { return String::new() };
    let mut end = first.len();
    for s in &v[1..] {
        end = end.min(first.bytes().zip(s.bytes()).take_while(|(a, b)| a == b).count());
    }
    while !first.is_char_boundary(end) {
        end -= 1;
    }
    first[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("lvc-complete-{}", std::process::id()));
        std::fs::create_dir_all(d.join("docs")).unwrap();
        std::fs::write(d.join("docs/readme.md"), "").unwrap();
        std::fs::write(d.join("docs/read me.txt"), "").unwrap();
        std::fs::write(d.join("data.csv"), "").unwrap();
        std::fs::write(d.join(".hidden"), "").unwrap();
        d
    }

    #[test]
    fn a_path_word_completes_against_the_directory() {
        let d = tmp();
        let (at, c) = candidates("cat da", &d);
        assert_eq!(at, 4);
        assert_eq!(c, vec!["data.csv"]);
        let (_, c) = candidates("cat docs/rea", &d);
        assert_eq!(c, vec!["docs/read\\ me.txt", "docs/readme.md"]);
        // a directory ends in a slash; dot files only when asked
        let (_, c) = candidates("ls d", &d);
        assert_eq!(c, vec!["data.csv", "docs/"]);
        assert!(candidates("ls ", &d).1.iter().all(|n| !n.starts_with('.')));
        assert_eq!(candidates("ls .h", &d).1, vec![".hidden"]);
    }

    #[test]
    fn the_first_word_is_a_command_the_next_a_path() {
        let d = tmp();
        let (at, c) = candidates("ec", &d);
        assert_eq!(at, 0);
        assert!(c.iter().any(|n| n == "echo"));
        // after `;` or `|` a command again; with a slash it is a path
        assert!(candidates("ls | gre", &d).1.iter().any(|n| n == "grep"));
        assert_eq!(candidates("./da", &d).1, vec!["./data.csv"]);
    }

    #[test]
    fn the_common_start() {
        assert_eq!(common_prefix(&["read me".into(), "readme".into()]), "read");
        assert_eq!(common_prefix(&["a".into()]), "a");
        assert_eq!(common_prefix(&[]), "");
    }
}
