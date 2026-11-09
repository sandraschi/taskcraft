//! Git history -> issue links, via the installed `git` binary (no libgit2).
//!
//! Only explicit closing phrases close an issue: `closes TC-12`, `fixes #TC-12`,
//! `resolved TC-12` (case-insensitive, whole words). Any other mention of
//! `TC-12` links the commit without changing status. This fixes the draft spec's
//! bug where any commit containing "fix" anywhere closed the issue.

use std::path::Path;
use std::process::Command;

const CLOSE_WORDS: &[&str] = &[
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    pub hash: String,
    pub author: String,
    pub committed_at: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub number: i64,
    pub closing: bool,
}

/// Find `KEY-n` mentions in a commit message.
pub fn mentions(message: &str, key: &str) -> Vec<Mention> {
    let key = key.to_ascii_uppercase();
    let tokens: Vec<String> = message
        .split(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    ',' | ';' | '(' | ')' | '[' | ']' | ':' | '.' | '!' | '?' | '"' | '\''
                )
        })
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    let mut out: Vec<Mention> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        let t = tok.trim_start_matches('#').to_ascii_uppercase();
        let Some(rest) = t.strip_prefix(&key) else {
            continue;
        };
        let Some(num) = rest.strip_prefix('-') else {
            continue;
        };
        if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) || num.len() > 9 {
            continue;
        }
        let Ok(number) = num.parse::<i64>() else {
            continue;
        };
        if number == 0 {
            continue;
        }
        let closing = i
            .checked_sub(1)
            .and_then(|p| tokens.get(p))
            .is_some_and(|prev| CLOSE_WORDS.contains(&prev.to_ascii_lowercase().as_str()));
        match out.iter_mut().find(|m| m.number == number) {
            Some(m) => m.closing |= closing,
            None => out.push(Mention { number, closing }),
        }
    }
    out
}

/// Parse `git log --format=%H%x1f%an%x1f%cI%x1f%s%x1f%b%x1e` output.
pub fn parse_log(raw: &str) -> Vec<GitCommit> {
    raw.split('\u{1e}')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches(['\n', '\r']);
            let mut f = rec.split('\u{1f}');
            let hash = f.next()?.trim().to_string();
            if hash.len() < 7 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            Some(GitCommit {
                hash,
                author: f.next().unwrap_or("").trim().to_string(),
                committed_at: f.next().unwrap_or("").trim().to_string(),
                subject: f.next().unwrap_or("").trim().to_string(),
                body: f.next().unwrap_or("").trim().to_string(),
            })
        })
        .collect()
}

/// Read up to `max` commits (newest first) from the repo at `path`.
pub fn read_log(path: &Path, max: usize) -> Result<Vec<GitCommit>, String> {
    let n = max.clamp(1, 5000).to_string();
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "log",
            "--no-color",
            "-n",
            &n,
            "--format=%H%x1f%an%x1f%cI%x1f%s%x1f%b%x1e",
        ])
        .output()
        .map_err(|e| format!("cannot run git: {e} (is git installed and on PATH?)"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git log failed: {}", err.trim()));
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_phrases_close() {
        let m = mentions("fix: handle WAL lock, see TC-12", "TC");
        assert_eq!(
            m,
            vec![Mention {
                number: 12,
                closing: false
            }]
        );
        let m = mentions(
            "Refactor parser. Closes #TC-12, fixes tc-7 (and TC-7 again)",
            "TC",
        );
        assert_eq!(
            m,
            vec![
                Mention {
                    number: 12,
                    closing: true
                },
                Mention {
                    number: 7,
                    closing: true
                }
            ]
        );
        assert!(mentions("ETC-12 is not ours, nor TC-x, nor TC-0", "TC").is_empty());
        assert_eq!(
            mentions("resolved: TC-3", "TC"),
            vec![Mention {
                number: 3,
                closing: true
            }]
        );
        assert_eq!(mentions("prefix fixesTC-3", "TC"), Vec::<Mention>::new());
    }

    #[test]
    fn parses_log_records() {
        let raw = "abcdef1234567\u{1f}Joe Mocky\u{1f}2026-11-09T10:00:00+01:00\u{1f}feat: board\u{1f}Closes TC-1\u{1e}\n\
                   0123456789abc\u{1f}Hannes\u{1f}2026-11-08T09:00:00+01:00\u{1f}chore\u{1f}\u{1e}\nnot-a-hash\u{1f}x\u{1e}";
        let c = parse_log(raw);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].body, "Closes TC-1");
        assert_eq!(c[1].author, "Hannes");
    }
}
