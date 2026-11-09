//! What git says of a run that talks to a remote while it runs.
//!
//! Given `--progress`, git writes how far each phase has got to its standard
//! error, rewriting one line with `\r` as it goes: `Receiving objects:  45%
//! (450/1000), 1.20 MiB | 2.30 MiB/s`, and the remote's own phases after
//! `remote: `. The words are git's and follow its language; only the shape -
//! a phase, a colon, a count or a percentage with a count - is read here, so a
//! git that speaks another language is read the same.

use serde::Serialize;

/// How far one phase of a run of git has got, as git tells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Progress {
    /// The phase, in git's own words: `Receiving objects`, `Writing objects`.
    pub phase: String,
    /// The remote is doing it - git wrote `remote: ` before it - rather than
    /// this machine.
    pub remote: bool,
    /// How much of it is done.
    pub done: u64,
    /// How much there is; `None` while git is still counting.
    pub total: Option<u64>,
    /// The phase is over: git wrote `done` after it.
    pub finished: bool,
}

impl Progress {
    /// How much of the phase is done, out of a hundred, when git knows the
    /// total.
    pub fn percent(&self) -> Option<u8> {
        let total = self.total.filter(|total| *total > 0)?;
        Some(((self.done.min(total) * 100) / total) as u8)
    }
}

/// The progress `segment` says, if it is a line of progress at all; one of
/// git's other words - `To origin`, `error: ...`, a hint - is not.
pub(crate) fn progress(segment: &str) -> Option<Progress> {
    let text = segment.trim();
    let (remote, text) = match text.strip_prefix("remote:") {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let (phase, rest) = text.split_once(": ")?;
    let phase = phase.trim();
    if phase.is_empty() || phase.contains(':') {
        return None;
    }
    let rest = rest.trim_start();
    let finished = rest.trim_end().ends_with("done.") || rest.contains(", done");
    // `45% (450/1000), ...` once the total is known, `1234, done.` before.
    let (done, total) = match rest.split_once('%') {
        Some((percent, after)) if number(percent.trim()).is_some() => {
            let counts = after.trim_start().strip_prefix('(')?;
            let (done, total) = counts.split_once('/')?;
            let total = total.split(')').next()?;
            (number(done.trim())?, Some(number(total.trim())?))
        }
        _ => (number(leading_digits(rest))?, None),
    };
    Some(Progress {
        phase: phase.to_owned(),
        remote,
        done,
        total,
        finished,
    })
}

/// The digits `text` starts with, which may be all of it.
fn leading_digits(text: &str) -> &str {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    &text[..end]
}

fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// What a terminal would show of `text` once git is done writing it: each
/// line as its last rewrite left it. git rewrites a line of progress with
/// `\r` dozens of times a second, and only the last of them is worth keeping;
/// a `\r\n` ends a line as `\n` does, and the spaces that only covered what
/// was there before go.
pub(crate) fn as_shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (at, line) in text.split('\n').enumerate() {
        if at > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        // The remote pads its lines with spaces to cover a longer one before.
        out.push_str(line.rsplit('\r').next().unwrap_or(line).trim_end());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Progress, as_shown, progress};

    fn seen(phase: &str, remote: bool, done: u64, total: Option<u64>, finished: bool) -> Progress {
        Progress {
            phase: phase.to_owned(),
            remote,
            done,
            total,
            finished,
        }
    }

    #[test]
    fn a_phase_with_a_total_is_read_with_its_counts() {
        assert_eq!(
            progress("Receiving objects:  45% (450/1000), 1.20 MiB | 2.30 MiB/s"),
            Some(seen("Receiving objects", false, 450, Some(1000), false))
        );
        assert_eq!(
            progress("Writing objects: 100% (3/3), 239 bytes | 239.00 KiB/s, done."),
            Some(seen("Writing objects", false, 3, Some(3), true))
        );
    }

    #[test]
    fn the_remotes_own_phases_and_a_count_without_a_total_are_read() {
        assert_eq!(
            progress("remote: Counting objects: 100% (5/5), done."),
            Some(seen("Counting objects", true, 5, Some(5), true))
        );
        assert_eq!(
            progress("remote: Enumerating objects: 1234, done."),
            Some(seen("Enumerating objects", true, 1234, None, true))
        );
        assert_eq!(
            progress("Counting objects: 87"),
            Some(seen("Counting objects", false, 87, None, false))
        );
    }

    #[test]
    fn a_phase_in_another_language_is_read_by_its_shape() {
        assert_eq!(
            progress("Получение объектов:  50% (1/2)"),
            Some(seen("Получение объектов", false, 1, Some(2), false))
        );
    }

    #[test]
    fn what_is_not_progress_is_not_taken_for_it() {
        for line in [
            "To ../origin.git",
            " * [new branch]      main -> main",
            "fatal: unable to access 'https://example.com/x.git/': Could not resolve host: example.com",
            "error: failed to push some refs to 'origin'",
            "hint: Updates were rejected because the remote contains work that you do not",
            "remote: ",
            "branch 'main' set up to track 'origin/main'.",
            "Total 3 (delta 0), reused 0 (delta 0), pack-reused 0",
            "",
        ] {
            assert_eq!(progress(line), None, "{line}");
        }
    }

    #[test]
    fn a_percentage_is_of_the_total_and_never_more_than_all() {
        assert_eq!(seen("x", false, 450, Some(1000), false).percent(), Some(45));
        assert_eq!(seen("x", false, 7, Some(5), false).percent(), Some(100));
        assert_eq!(seen("x", false, 7, None, false).percent(), None);
        assert_eq!(seen("x", false, 0, Some(0), false).percent(), None);
    }

    #[test]
    fn a_rewritten_line_is_kept_as_it_was_left() {
        let written = "Counting objects:  33% (1/3)        \rCounting objects: 100% (3/3)\rCounting objects: 100% (3/3), done.        \nTo ../o.git\r\n * [new branch]      main -> main\n";
        assert_eq!(
            as_shown(written),
            "Counting objects: 100% (3/3), done.\nTo ../o.git\n * [new branch]      main -> main\n"
        );
        assert_eq!(as_shown("plain words"), "plain words");
    }
}
