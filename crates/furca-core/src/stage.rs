//! Taking one hunk of a file into the index, or out of it again.
//!
//! The hunk becomes a patch, and `git apply --cached` puts it in: the change
//! goes through git like every other (ADR 0001), and git refuses a patch that
//! does not fit what the index holds, so a list that went stale cannot put
//! lines where they do not belong. The patch is written from the bytes of both
//! sides, never from the text the window shows, which has lost whatever was not
//! UTF-8. See ADR 0019.

use std::io::Write;

use crate::Repository;
use crate::diff::{FileChange, FileStatus, Hunk, LineRange};
use crate::error::{Error, Source};

/// Unchanged lines put around a hunk, as `git diff` does.
const CONTEXT: usize = 3;

impl Repository {
    /// Stages `hunk`, one of the hunks [`Repository::diff`] gives for `file`
    /// between the index and the working tree; the rest of the file stays as it
    /// was. A hunk the file no longer has is [`Error::Moved`].
    pub fn stage_hunk(&self, file: &FileChange, hunk: Hunk) -> Result<(), Error> {
        let (index, tree) = self.sides_of(file, hunk, Self::working_tree_cache)?;
        self.git()
            .apply_to_index(&patch_of(&file.path, &index, &tree, hunk))
    }

    /// Takes `hunk`, one of the hunks [`Repository::diff`] gives for `file`
    /// between `HEAD` and the index, out of the index again; the working tree is
    /// not touched. A hunk the file no longer has is [`Error::Moved`].
    pub fn unstage_hunk(&self, file: &FileChange, hunk: Hunk) -> Result<(), Error> {
        let (head, index) = self.sides_of(file, hunk, |repo| {
            repo.inner
                .diff_resource_cache_for_tree_diff()
                .map_err(Into::into)
        })?;
        let back = Hunk {
            before: hunk.after,
            after: hunk.before,
        };
        self.git()
            .apply_to_index(&patch_of(&file.path, &index, &head, back))
    }

    /// The bytes of both sides of `file`, once it is known `hunk` is still one
    /// of its hunks.
    fn sides_of(
        &self,
        file: &FileChange,
        hunk: Hunk,
        cache: impl FnOnce(&Self) -> Result<gix::diff::blob::Platform, Source>,
    ) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let whole = || Error::NotByHunks {
            path: file.path.clone(),
        };
        if file.status != FileStatus::Modified {
            return Err(whole());
        }
        let mut cache = cache(self).map_err(|source| Error::Diff {
            path: file.path.clone(),
            source,
        })?;
        let compared = self.compare_keeping(&mut cache, file, true)?;
        let Some(bytes) = compared.bytes else {
            return Err(whole());
        };
        if !compared.hunks.contains(&hunk) {
            return Err(Error::Moved {
                path: file.path.clone(),
            });
        }
        Ok(bytes)
    }
}

/// A patch that turns `old` into `old` with only `hunk` changed to the lines of
/// `new` it names. Lines are bytes up to and including a `\n`.
pub(crate) fn patch_of(path: &str, old: &[u8], new: &[u8], hunk: Hunk) -> Vec<u8> {
    let old: Vec<&[u8]> = old.split_inclusive(|byte| *byte == b'\n').collect();
    let new: Vec<&[u8]> = new.split_inclusive(|byte| *byte == b'\n').collect();
    let Hunk { before, after } = fitted(hunk, &old);

    let (start, end) = (
        before.start as usize,
        (before.start + before.count) as usize,
    );
    let lead = start.saturating_sub(CONTEXT);
    let tail = (end + CONTEXT).min(old.len());
    let added = &new[after.start as usize..(after.start + after.count) as usize];
    let kept = (start - lead) + (tail - end);

    let mut out = Vec::new();
    let (a, b) = (quoted("a/", path), quoted("b/", path));
    out.extend_from_slice(b"diff --git ");
    out.extend_from_slice(&a);
    out.push(b' ');
    out.extend_from_slice(&b);
    out.extend_from_slice(b"\n--- ");
    out.extend_from_slice(&a);
    out.extend_from_slice(b"\n+++ ");
    out.extend_from_slice(&b);
    out.push(b'\n');
    // The hunks before this one are not in the patch, so both sides start at
    // the same line. A side with no lines names the line before it, as git does.
    let at = |count: usize| if count == 0 { lead } else { lead + 1 };
    let (old_count, new_count) = (kept + before.count as usize, kept + added.len());
    let _ = writeln!(
        out,
        "@@ -{},{old_count} +{},{new_count} @@",
        at(old_count),
        at(new_count)
    );
    for line in &old[lead..start] {
        line_of(&mut out, b' ', line);
    }
    for line in &old[start..end] {
        line_of(&mut out, b'-', line);
    }
    for line in added {
        line_of(&mut out, b'+', line);
    }
    for line in &old[end..tail] {
        line_of(&mut out, b' ', line);
    }
    out
}

/// The hunk as a patch can say it. Lines are compared without their `\n`, so a
/// last line that gained one is no change to the comparison, and lines added
/// after it would read as added to the end of it, with no newline between. Such
/// a line is said as changed.
fn fitted(hunk: Hunk, old: &[&[u8]]) -> Hunk {
    let Hunk { before, after } = hunk;
    let at_the_end = before.count == 0 && before.start as usize == old.len();
    let unended = old.last().is_some_and(|line| !line.ends_with(b"\n"));
    if at_the_end && unended && after.start > 0 {
        let wider = |range: LineRange| LineRange {
            start: range.start - 1,
            count: range.count + 1,
        };
        return Hunk {
            before: wider(before),
            after: wider(after),
        };
    }
    hunk
}

fn line_of(out: &mut Vec<u8>, mark: u8, line: &[u8]) {
    out.push(mark);
    out.extend_from_slice(line);
    if !line.ends_with(b"\n") {
        out.extend_from_slice(b"\n\\ No newline at end of file\n");
    }
}

/// `prefix` and `path` as a patch names a file: plain when it can be, and
/// otherwise quoted as git quotes it, with anything else as an octal escape, so
/// a space, a quote or a name in another alphabet all read back as themselves.
fn quoted(prefix: &str, path: &str) -> Vec<u8> {
    let name = format!("{prefix}{path}");
    let plain = |byte: u8| byte.is_ascii_alphanumeric() || b"._/+@=,-".contains(&byte);
    if name.bytes().all(plain) {
        return name.into_bytes();
    }
    let mut out = vec![b'"'];
    for byte in name.bytes() {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            0x20..=0x7e => out.push(byte),
            _ => {
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out.push(b'"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: u32, count: u32) -> LineRange {
        LineRange { start, count }
    }

    fn hunk(before: (u32, u32), after: (u32, u32)) -> Hunk {
        Hunk {
            before: range(before.0, before.1),
            after: range(after.0, after.1),
        }
    }

    fn text(patch: &[u8]) -> String {
        String::from_utf8_lossy(patch).into_owned()
    }

    #[test]
    fn a_patch_says_only_its_hunk_with_the_lines_around_it() {
        let old = b"1\n2\n3\n4\n5\n6\n7\n8\n";
        let new = b"1\nTWO\n3\n4\n5\n6\nSEVEN\n8\n";
        assert_eq!(
            text(&patch_of("a.txt", old, new, hunk((6, 1), (6, 1)))),
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n\
             @@ -4,5 +4,5 @@\n 4\n 5\n 6\n-7\n+SEVEN\n 8\n"
        );
    }

    #[test]
    fn a_line_with_no_newline_says_so_on_whichever_side_it_is() {
        let patch = text(&patch_of("a", b"x\ny", b"x\nz\n", hunk((1, 1), (1, 1))));
        assert!(
            patch.ends_with(" x\n-y\n\\ No newline at end of file\n+z\n"),
            "{patch}"
        );
        let patch = text(&patch_of("a", b"x\ny\n", b"x\nz", hunk((1, 1), (1, 1))));
        assert!(
            patch.ends_with(" x\n-y\n+z\n\\ No newline at end of file\n"),
            "{patch}"
        );
    }

    #[test]
    fn lines_added_after_a_last_line_with_no_newline_end_that_line_first() {
        let patch = text(&patch_of("a", b"x\ny", b"x\ny\nz\n", hunk((2, 0), (2, 1))));
        assert!(
            patch.ends_with("@@ -1,2 +1,3 @@\n x\n-y\n\\ No newline at end of file\n+y\n+z\n"),
            "{patch}"
        );
    }

    #[test]
    fn a_name_git_would_quote_is_quoted() {
        assert_eq!(quoted("a/", "src/main.rs"), b"a/src/main.rs");
        assert_eq!(quoted("a/", "my file.txt"), b"\"a/my file.txt\"");
        assert_eq!(quoted("b/", "q\"x"), b"\"b/q\\\"x\"");
        assert_eq!(quoted("a/", "é"), b"\"a/\\303\\251\"");
    }
}
