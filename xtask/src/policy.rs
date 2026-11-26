//! Rule #1, statically: the repository only ever contains small text files
//! that we wrote. See docs/asset-policy.md.
//!
//! This is an allowlist, not a blocklist: a file passes only if its name is
//! on the list, it is not too big, it is text, and it does not smuggle data
//! inside text (long hex/base64 runs).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

const MAX_FILE_SIZE: usize = 512 * 1024;
const MAX_ENCODED_RUN: usize = 512;

const ALLOWED_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "txt", "lock", "yml", "yaml", "json", "hlsl", "hlsli", "ps1", "cmd", "sh",
];
const ALLOWED_NAMES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".victory-root",
    "pre-commit",
    "LICENSE",
    "LICENSE-MIT",
    "LICENSE-APACHE",
];

/// Paths explicitly exempted, with a reason. Keep this empty if at all possible.
const EXCEPTIONS: &[(&str, &str)] = &[(
    "crates/victory-formats/src/bcpack-tables.bin",
    "the BCPack format's constant tables: nothing decodes without them and they cannot be derived (owner's decision, 2026-11-23; docs/asset-policy.md)",
)];

/// Run logs: the layer's own log of a run, scrubbed of names, kept under
/// `diagnostics/` by the owner's decision (2026-11-26; docs/asset-policy.md).
/// A log is text we wrote, but long: these may be this large as text, and
/// the longest are kept gzipped ([`RUN_LOG_GZ_MAX`]).
const RUN_LOG_MAX: usize = 32 * 1024 * 1024;
/// A gzipped run log (`diagnostics/*.txt.gz`) may be this large.
const RUN_LOG_GZ_MAX: usize = 16 * 1024 * 1024;

/// Whether `path` is a run log directly under `diagnostics/`: `Some(true)`
/// for a gzipped one (`.txt.gz`), `Some(false)` for a text one (`.txt`).
fn run_log(path: &str) -> Option<bool> {
    let name = path.strip_prefix("diagnostics/")?;
    if name.contains('/') {
        return None;
    }
    if name.ends_with(".txt.gz") {
        Some(true)
    } else if name.ends_with(".txt") {
        Some(false)
    } else {
        None
    }
}

/// Signatures of formats that must never be committed.
const FORBIDDEN_MAGIC: &[(&[u8], &str)] = &[
    (b"MZ", "PE executable"),
    (b"DXBC", "shader container"),
    (b"DDS ", "DDS texture"),
    (b"RIFF", "RIFF (audio/video)"),
    (b"BC\xC0\xDE", "LLVM bitcode"),
    (b"Microsoft C/C++ MSF", "PDB"),
    (b"!<arch>", "library archive"),
    (b"PK\x03\x04", "zip"),
    (b"7z\xBC\xAF", "7-zip"),
    (b"MSCF", "cabinet"),
    (b"\xD0\xCF\x11\xE0", "MSI/OLE"),
];

pub fn run(root: &Path, staged: bool) -> Result<()> {
    let files = list_files(root, staged)?;
    let mut violations = Vec::new();
    for path in &files {
        if EXCEPTIONS.iter().any(|(p, _)| p == path) {
            continue;
        }
        let content = if staged {
            let out = Command::new("git")
                .args(["show", &format!(":{path}")])
                .current_dir(root)
                .output()?;
            if !out.status.success() {
                continue; // deleted in the index
            }
            out.stdout
        } else {
            match std::fs::read(root.join(path)) {
                Ok(c) => c,
                Err(_) => continue,
            }
        };
        for problem in check(path, &content) {
            violations.push(format!("{path}: {problem}"));
        }
    }
    if violations.is_empty() {
        println!("asset policy: {} files ok", files.len());
        return Ok(());
    }
    for v in &violations {
        eprintln!("POLICY: {v}");
    }
    bail!(
        "{} asset-policy violation(s). Nothing from the game may be committed; \
         see docs/asset-policy.md",
        violations.len()
    )
}

fn list_files(root: &Path, staged: bool) -> Result<Vec<String>> {
    let git = |args: &[&str]| -> Result<Option<Vec<String>>> {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .context("running git")?;
        if !out.status.success() {
            return Ok(None);
        }
        Ok(Some(
            out.stdout
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).replace('\\', "/"))
                .collect(),
        ))
    };
    if staged {
        return git(&[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACMR",
            "-z",
        ])?
        .context("--staged needs a git repository");
    }
    if let Some(mut tracked) = git(&[
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ])? {
        tracked.sort();
        tracked.dedup();
        return Ok(tracked);
    }
    // Not a git repository: walk everything except build output.
    Ok(walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !matches!(e.file_name().to_str(), Some("target" | ".git")))
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            e.path()
                .strip_prefix(root)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect())
}

/// All problems with one file. Pure, for testing.
pub fn check(path: &str, content: &[u8]) -> Vec<String> {
    let mut problems = Vec::new();
    // A run log has rules of its own: a larger size, and for a gzipped one
    // the gzip signature instead of the text rule. (No rule on hex runs: a
    // log quotes hashes and addresses.)
    if let Some(gzipped) = run_log(path) {
        let limit = if gzipped { RUN_LOG_GZ_MAX } else { RUN_LOG_MAX };
        if content.len() > limit {
            problems.push(format!("{} bytes exceeds the {limit}-byte limit for a run log", content.len()));
        }
        if gzipped {
            if !content.starts_with(&[0x1f, 0x8b]) {
                problems.push("a .txt.gz run log that is not a gzip stream".to_owned());
            }
        } else {
            for (magic, what) in FORBIDDEN_MAGIC {
                if content.starts_with(magic) {
                    problems.push(format!("looks like a {what}"));
                }
            }
            if content.contains(&0) {
                problems.push("binary content (NUL bytes)".to_owned());
            }
        }
        return problems;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let allowed = ALLOWED_NAMES.contains(&name)
        || ext
            .as_deref()
            .is_some_and(|e| ALLOWED_EXTENSIONS.contains(&e));
    if !allowed {
        problems.push("file type not on the allowlist".to_owned());
    }
    if content.len() > MAX_FILE_SIZE {
        problems.push(format!(
            "{} bytes exceeds the {MAX_FILE_SIZE}-byte limit",
            content.len()
        ));
    }
    for (magic, what) in FORBIDDEN_MAGIC {
        if content.starts_with(magic) {
            problems.push(format!("looks like a {what}"));
        }
    }
    if content.contains(&0) {
        problems.push("binary content (NUL bytes)".to_owned());
    } else if let Some(run) = longest_encoded_run(content)
        && run > MAX_ENCODED_RUN
    {
        problems.push(format!("{run}-character hex/base64 run (embedded data?)"));
    }
    problems
}

/// Longest run of characters that could be hex or base64 payload.
fn longest_encoded_run(content: &[u8]) -> Option<usize> {
    let is_payload = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=');
    content.split(|b| !is_payload(b)).map(<[u8]>::len).max()
}

#[cfg(test)]
mod tests {
    use super::check;

    /// Enforces the policy wherever tests run, even in clones without the hook.
    #[test]
    fn repository_passes_policy() {
        super::run(&crate::workspace_root(), false).unwrap();
    }

    /// Run logs under `diagnostics/` may be long, and gzipped; nothing else
    /// there, nothing deeper, and nothing that is not what its name says.
    #[test]
    fn run_logs_have_their_own_rules() {
        let long = vec![b'a'; 4 * 1024 * 1024];
        assert!(check("diagnostics/run-1.txt", &long).is_empty());
        assert!(check("diagnostics/run-1.txt.gz", &[0x1f, 0x8b, 8, 0, 0, 0]).is_empty());
        // Not a gzip stream; binary under a text name; too long.
        assert!(!check("diagnostics/run-1.txt.gz", b"MZ\x90\x00").is_empty());
        assert!(!check("diagnostics/run-1.txt", b"abc\0def").is_empty());
        assert!(!check("diagnostics/run-1.txt", &vec![b'a'; super::RUN_LOG_MAX + 1]).is_empty());
        assert!(!check("diagnostics/run-1.txt.gz", &[&[0x1f, 0x8b][..], &vec![0u8; super::RUN_LOG_GZ_MAX][..]].concat()).is_empty());
        // The ordinary rules everywhere else, and for other names there.
        assert!(!check("docs/run-1.txt", &long).is_empty());
        assert!(!check("diagnostics/deeper/run-1.txt", &long).is_empty());
        assert!(!check("diagnostics/shot.png", b"\x89PNG").is_empty());
    }

    #[test]
    fn accepts_source() {
        assert!(check("crates/x/src/lib.rs", b"fn main() {}\n").is_empty());
        assert!(check(".gitignore", b"/target/\n").is_empty());
        assert!(check(".githooks/pre-commit", b"#!/bin/sh\n").is_empty());
    }

    #[test]
    fn rejects_game_like_files() {
        assert!(!check("assets/tex.dds", b"DDS \x7c\0\0\0").is_empty());
        assert!(
            !check("notes.txt", b"MZ\x90\0").is_empty(),
            "magic wins over extension"
        );
        assert!(
            !check("data.bin", b"hello").is_empty(),
            "extension not allowed"
        );
        assert!(
            !check("a.rs", &vec![b'a'; 600 * 1024]).is_empty(),
            "too big"
        );
    }

    #[test]
    fn rejects_data_smuggled_into_text() {
        let blob = format!("const DATA: &str = \"{}\";", "A1".repeat(400));
        assert!(!check("src/blob.rs", blob.as_bytes()).is_empty());
    }
}
