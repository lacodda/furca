//! Taking lines of a file into the index, out of it again, or out of the
//! working tree.
//!
//! The lines become a patch, and `git apply` puts it in: the change goes
//! through git like every other (ADR 0001), and git refuses a patch that does
//! not fit what it is applied to, so a list that went stale cannot put lines
//! where they do not belong. The patch is written from the bytes of both sides,
//! never from the text the window shows, which has lost whatever was not UTF-8.
//! See ADR 0019 and ADR 0020.

use std::collections::BTreeMap;
use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::diff::{FileChange, FileStatus, Hunk, LineRange};
use crate::error::{Error, Source};

/// Unchanged lines put around a hunk, as `git diff` does.
const CONTEXT: usize = 3;

/// Lines of one hunk to take: which of the lines it removes, and which of the
/// lines it adds, each counted from 0 in its own text as the hunk's ranges are.
/// A hunk taken whole is [`Picked::whole`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Picked {
    pub hunk: Hunk,
    /// Lines of the text before, inside `hunk.before`: the ones left out stay.
    pub removed: Vec<u32>,
    /// Lines of the text after, inside `hunk.after`: the ones left out are not
    /// added.
    pub added: Vec<u32>,
}

impl Picked {
    /// Every line of `hunk`.
    pub fn whole(hunk: Hunk) -> Picked {
        let all = |range: LineRange| (range.start..range.start + range.count).collect();
        Picked {
            hunk,
            removed: all(hunk.before),
            added: all(hunk.after),
        }
    }

    /// Every line it names is inside its hunk.
    fn fits(&self) -> bool {
        let inside = |lines: &[u32], range: LineRange| {
            lines
                .iter()
                .all(|line| (range.start..range.start + range.count).contains(line))
        };
        inside(&self.removed, self.hunk.before) && inside(&self.added, self.hunk.after)
    }
}

impl Repository {
    /// Stages the lines `picks` name, of the hunks [`Repository::diff`] gives
    /// for `file` between the index and the working tree; the rest of the file
    /// stays as it was. A hunk or a line the file no longer has is
    /// [`Error::Moved`], and nothing is staged.
    pub fn stage_lines(&self, file: &FileChange, picks: &[Picked]) -> Result<(), Error> {
        let (index, tree) = self.sides_of(file, picks, Self::working_tree_cache)?;
        let taken = picks.iter().map(Taken::forward);
        match patch_of(&file.path, &index, &tree, taken) {
            Some(patch) => self.git().apply_to_index(&patch),
            None => Ok(()),
        }
    }

    /// Takes the lines `picks` name, of the hunks [`Repository::diff`] gives
    /// for `file` between `HEAD` and the index, out of the index again; the
    /// working tree is not touched. A hunk or a line the file no longer has is
    /// [`Error::Moved`].
    pub fn unstage_lines(&self, file: &FileChange, picks: &[Picked]) -> Result<(), Error> {
        let (head, index) = self.sides_of(file, picks, |repo| {
            repo.inner
                .diff_resource_cache_for_tree_diff()
                .map_err(Into::into)
        })?;
        let taken = picks.iter().map(Taken::back);
        match patch_of(&file.path, &index, &head, taken) {
            Some(patch) => self.git().apply_to_index(&patch),
            None => Ok(()),
        }
    }

    /// Puts the lines `picks` name, of the hunks [`Repository::diff`] gives
    /// for `file` between the index and the working tree, back in the working
    /// tree as the index has them: what they changed is gone, and git keeps no
    /// copy of it. A hunk or a line the file no longer has is [`Error::Moved`],
    /// and the file stays as it was.
    pub fn discard_lines(&self, file: &FileChange, picks: &[Picked]) -> Result<(), Error> {
        let (index, tree) = self.sides_of(file, picks, Self::working_tree_cache)?;
        let taken = picks.iter().map(Taken::back);
        match patch_of(&file.path, &tree, &index, taken) {
            Some(patch) => self.git().apply_to_working_tree(&patch),
            None => Ok(()),
        }
    }

    /// The bytes of both sides of `file`, once it is known every hunk `picks`
    /// name is still one of its hunks, and every line inside its hunk.
    fn sides_of(
        &self,
        file: &FileChange,
        picks: &[Picked],
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
        let current = |pick: &Picked| compared.hunks.contains(&pick.hunk) && pick.fits();
        if !picks.iter().all(current) {
            return Err(Error::Moved {
                path: file.path.clone(),
            });
        }
        Ok(bytes)
    }
}

/// Lines of one hunk as a patch takes them, from an old text to a new one: the
/// old lines to remove, and the new lines to add.
#[derive(Debug, Clone)]
pub(crate) struct Taken {
    before: LineRange,
    after: LineRange,
    removed: Vec<u32>,
    added: Vec<u32>,
}

impl Taken {
    /// From the text before to the text after, as the hunk was read.
    fn forward(pick: &Picked) -> Taken {
        Taken {
            before: pick.hunk.before,
            after: pick.hunk.after,
            removed: pick.removed.clone(),
            added: pick.added.clone(),
        }
    }

    /// From the text after back to the text before: the lines it added are the
    /// ones to remove, and the ones it removed are the ones to put back.
    fn back(pick: &Picked) -> Taken {
        Taken {
            before: pick.hunk.after,
            after: pick.hunk.before,
            removed: pick.added.clone(),
            added: pick.removed.clone(),
        }
    }

    /// The old line after its last one.
    fn end(&self) -> usize {
        (self.before.start + self.before.count) as usize
    }
}

/// A line of a patch: `-`, `+` or a space, and the line with its `\n`, if any.
type Marked = (u8, Vec<u8>);

/// A patch that changes `old` only where `taken` says: the old lines it names
/// removed, the new lines it names added, every other line of `old` kept as it
/// is. Lines are bytes up to and including a `\n`. `None` when it names no line
/// at all, and there is nothing to apply.
pub(crate) fn patch_of(
    path: &str,
    old: &[u8],
    new: &[u8],
    taken: impl IntoIterator<Item = Taken>,
) -> Option<Vec<u8>> {
    let old: Vec<&[u8]> = old.split_inclusive(|byte| *byte == b'\n').collect();
    let new: Vec<&[u8]> = new.split_inclusive(|byte| *byte == b'\n').collect();

    // One hunk named twice is the lines of both.
    let mut by_start: BTreeMap<u32, Taken> = BTreeMap::new();
    for each in taken {
        if each.removed.is_empty() && each.added.is_empty() {
            continue;
        }
        match by_start.get_mut(&each.before.start) {
            Some(known) if known.before == each.before && known.after == each.after => {
                known.removed.extend(each.removed);
                known.added.extend(each.added);
            }
            _ => {
                by_start.insert(each.before.start, each);
            }
        }
    }
    let taken: Vec<Taken> = by_start.into_values().collect();
    if taken.is_empty() {
        return None;
    }

    // Hunks whose context would meet go in one, as `git diff` writes them: a
    // second hunk whose context said lines the first had changed would not fit.
    let mut groups: Vec<Vec<&Taken>> = Vec::new();
    for each in &taken {
        match groups.last_mut() {
            Some(group)
                if (each.before.start as usize)
                    <= group.last().map_or(0, |last| last.end()) + 2 * CONTEXT =>
            {
                group.push(each);
            }
            _ => groups.push(vec![each]),
        }
    }

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

    // How many lines the hunks before this one added, less the ones they
    // removed: where the new side of this one starts.
    let mut moved: isize = 0;
    for group in groups {
        let first = group[0].before.start as usize;
        let last = group[group.len() - 1].end();
        let lead = first.saturating_sub(CONTEXT);
        let tail = (last + CONTEXT).min(old.len());

        let mut lines: Vec<Marked> = Vec::new();
        let kept = |lines: &mut Vec<Marked>, range: std::ops::Range<usize>| {
            lines.extend(old[range].iter().map(|line| (b' ', line.to_vec())));
        };
        kept(&mut lines, lead..first);
        for (at, each) in group.iter().enumerate() {
            // Line by line, each old line with the new one in its place: the
            // second line of a block taken and the first left out reads as the
            // first line changed, not as the new line put after the old one.
            let (removes, adds) = (each.before.count, each.after.count);
            for k in 0..removes.max(adds) {
                if k < removes {
                    let line = each.before.start + k;
                    let mark = if each.removed.contains(&line) {
                        b'-'
                    } else {
                        b' '
                    };
                    lines.push((mark, old[line as usize].to_vec()));
                }
                let line = each.after.start + k;
                if k < adds && each.added.contains(&line) {
                    lines.push((b'+', new[line as usize].to_vec()));
                }
            }
            let next = group
                .get(at + 1)
                .map_or(tail, |next| next.before.start as usize);
            kept(&mut lines, each.end()..next);
        }
        let lines = ended(lines);

        let old_count = lines.iter().filter(|(mark, _)| *mark != b'+').count();
        let new_count = lines.iter().filter(|(mark, _)| *mark != b'-').count();
        // A side with no lines names the line before it, as git does.
        let old_start = if old_count == 0 { lead } else { lead + 1 };
        let new_from = lead.saturating_add_signed(moved);
        let new_start = if new_count == 0 {
            new_from
        } else {
            new_from + 1
        };
        let _ = writeln!(
            out,
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@"
        );
        for (mark, line) in &lines {
            line_of(&mut out, *mark, line);
        }
        moved += new_count as isize - old_count as isize;
    }
    Some(out)
}

/// The lines, with a line of the result that has no newline ended when more of
/// the result comes after it: the next line would otherwise read as more of
/// it, with nothing between. Only a last line has no newline, and lines are
/// compared without their `\n`, so one that gains it is no change to a hunk:
/// a kept one has to be said as changed here, and an added one ended.
fn ended(lines: Vec<Marked>) -> Vec<Marked> {
    let last = lines.iter().rposition(|(mark, _)| *mark != b'-');
    let mut out = Vec::with_capacity(lines.len() + 1);
    for (at, (mark, mut line)) in lines.into_iter().enumerate() {
        let followed = last.is_some_and(|last| at < last);
        if mark == b'-' || !followed || line.ends_with(b"\n") {
            out.push((mark, line));
            continue;
        }
        if mark == b' ' {
            out.push((b'-', line.clone()));
        }
        line.push(b'\n');
        out.push((b'+', line));
    }
    out
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

    fn whole(before: (u32, u32), after: (u32, u32)) -> Taken {
        Taken::forward(&Picked::whole(hunk(before, after)))
    }

    fn some(before: (u32, u32), after: (u32, u32), removed: &[u32], added: &[u32]) -> Taken {
        Taken::forward(&Picked {
            hunk: hunk(before, after),
            removed: removed.to_vec(),
            added: added.to_vec(),
        })
    }

    fn text(patch: Option<Vec<u8>>) -> String {
        String::from_utf8_lossy(&patch.expect("a patch")).into_owned()
    }

    #[test]
    fn a_patch_says_only_its_hunk_with_the_lines_around_it() {
        let old = b"1\n2\n3\n4\n5\n6\n7\n8\n";
        let new = b"1\nTWO\n3\n4\n5\n6\nSEVEN\n8\n";
        assert_eq!(
            text(patch_of("a.txt", old, new, [whole((6, 1), (6, 1))])),
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n\
             @@ -4,5 +4,5 @@\n 4\n 5\n 6\n-7\n+SEVEN\n 8\n"
        );
    }

    #[test]
    fn a_line_left_out_of_what_a_hunk_removes_stays_and_one_it_adds_is_not_added() {
        let old = b"1\n2\n3\n4\n";
        let new = b"1\nTWO\nTHREE\n4\n";
        // 2 becomes TWO, and 3 stays where it was: after it, not before.
        assert_eq!(
            text(patch_of("a", old, new, [some((1, 2), (1, 2), &[1], &[1])])),
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n\
             @@ -1,4 +1,4 @@\n 1\n-2\n+TWO\n 3\n 4\n"
        );
        // Only the removal of 2 and the addition of THREE.
        assert_eq!(
            text(patch_of("a", old, new, [some((1, 2), (1, 2), &[1], &[2])])),
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n\
             @@ -1,4 +1,4 @@\n 1\n-2\n 3\n+THREE\n 4\n"
        );
    }

    #[test]
    fn an_added_last_line_with_no_newline_is_ended_when_a_kept_line_comes_after_it() {
        // a and b become X, which has no newline; only X is taken, so a and b
        // stay, and b comes after it.
        let patch = text(patch_of(
            "a",
            b"a\nb\n",
            b"X",
            [some((0, 2), (0, 1), &[], &[0])],
        ));
        assert!(patch.ends_with("@@ -1,2 +1,3 @@\n a\n+X\n b\n"), "{patch}");
    }

    #[test]
    fn hunks_whose_lines_around_them_would_meet_go_as_one() {
        let old = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = b"1\nTWO\n3\n4\n5\n6\n7\nEIGHT\n9\n10\n";
        assert_eq!(
            text(patch_of(
                "a",
                old,
                new,
                [whole((7, 1), (7, 1)), whole((1, 1), (1, 1))]
            )),
            "diff --git a/a b/a\n--- a/a\n+++ b/a\n\
             @@ -1,10 +1,10 @@\n 1\n-2\n+TWO\n 3\n 4\n 5\n 6\n 7\n-8\n+EIGHT\n 9\n 10\n"
        );
    }

    #[test]
    fn a_hunk_apart_starts_where_the_lines_before_it_moved_it() {
        let old: Vec<u8> = (1..=20)
            .flat_map(|n| format!("{n}\n").into_bytes())
            .collect();
        let mut new = b"1\nONE-AND-A-HALF\n".to_vec();
        new.extend((2..=17).flat_map(|n| format!("{n}\n").into_bytes()));
        new.extend_from_slice(b"EIGHTEEN\n19\n20\n");
        let patch = text(patch_of(
            "a",
            &old,
            &new,
            [whole((1, 0), (1, 1)), whole((17, 1), (18, 1))],
        ));
        assert!(patch.contains("@@ -1,4 +1,5 @@\n"), "{patch}");
        assert!(patch.contains("@@ -15,6 +16,6 @@\n"), "{patch}");
    }

    #[test]
    fn nothing_named_is_no_patch() {
        assert!(patch_of("a", b"1\n", b"2\n", [some((0, 1), (0, 1), &[], &[])]).is_none());
        assert!(patch_of("a", b"1\n", b"2\n", []).is_none());
    }

    #[test]
    fn a_line_with_no_newline_says_so_on_whichever_side_it_is() {
        let patch = text(patch_of("a", b"x\ny", b"x\nz\n", [whole((1, 1), (1, 1))]));
        assert!(
            patch.ends_with(" x\n-y\n\\ No newline at end of file\n+z\n"),
            "{patch}"
        );
        let patch = text(patch_of("a", b"x\ny\n", b"x\nz", [whole((1, 1), (1, 1))]));
        assert!(
            patch.ends_with(" x\n-y\n+z\n\\ No newline at end of file\n"),
            "{patch}"
        );
    }

    #[test]
    fn lines_added_after_a_last_line_with_no_newline_end_that_line_first() {
        let patch = text(patch_of(
            "a",
            b"x\ny",
            b"x\ny\nz\n",
            [whole((2, 0), (2, 1))],
        ));
        assert!(
            patch.ends_with("@@ -1,2 +1,3 @@\n x\n-y\n\\ No newline at end of file\n+y\n+z\n"),
            "{patch}"
        );
        // The same when the last line is one the hunk would remove, but left out.
        let patch = text(patch_of(
            "a",
            b"x\ny",
            b"x\nz\n",
            [some((1, 1), (1, 1), &[], &[1])],
        ));
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
