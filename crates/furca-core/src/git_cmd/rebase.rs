//! Rebasing - onto a branch, or by a plan that reorders, rewords, folds and
//! drops commits - squashing the last commits into one, and absorbing a
//! change into a commit that is already made: each a run of the system `git`,
//! or a few that are one step to a person (ADR 0024).
//!
//! git asks for an interactive rebase's plan in an editor, and for a reworded
//! message in another. Here the plan is written before git runs and handed to
//! it by a sequence editor that is one line of shell - git runs its editors
//! through its own `sh` on every system - and a new message is a file an
//! `exec` line of the plan commits. No editor opens, and nothing waits for a
//! person.
//!
//! A squash and an absorb either happen whole or not at all: one that would
//! stop halfway is put back, and says why.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::operation::short;
use super::{Git, for_a_hook};
use crate::diff::FileChange;
use crate::error::Error;
use crate::operation::Stepped;
use crate::rebase::{RebaseAction, RebasePlan, RebaseTodo};
use crate::stage::Picked;
use crate::standing::{Operation, operation};

/// The folder in the git directory that holds what a rebase begun here reads
/// as it goes: its plan, and the messages of the commits it rewords. Taken
/// away by the first step that finds no rebase halfway.
pub(super) const KEPT: &str = "furca-rebase";

impl Git<'_> {
    /// Replays the commits `HEAD` has and `onto` does not on `onto` - a
    /// branch, a remote-tracking branch, a tag, a commit - and moves the
    /// branch there. Changes not committed are put away first and brought
    /// back after, by git's own autostash. One that meets a conflict stops
    /// halfway, and says so.
    pub fn rebase(&self, onto: &str) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let onto = self.repo.rebase_onto(onto)?.to_string();
        self.step(&["rebase", "--autostash", &onto])
    }

    /// Replays the commits [`Repository::rebase_plan`](crate::Repository::rebase_plan)
    /// lists for `onto` as `todo` says: in its order, each picked, reworded,
    /// folded into the one before it, or dropped. `todo` names every commit
    /// of the plan once - one it leaves out is refused, not dropped - and
    /// none that is not in it.
    pub fn rebase_interactive(
        &self,
        onto: Option<&str>,
        todo: &[RebaseTodo],
    ) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let plan = self.repo.rebase_plan(onto)?;
        if plan.steps.is_empty() {
            return Err(Error::Operation(
                "HEAD has no commit to replay there: there is nothing to rebase".to_owned(),
            ));
        }
        let kept = self.repo.git_dir().join(KEPT);
        let _ = std::fs::remove_dir_all(&kept);
        std::fs::create_dir_all(&kept).map_err(|source| writing(&kept, source))?;

        let lines = self.todo_lines(&plan, todo, &kept)?;
        let file = kept.join("todo");
        std::fs::write(&file, lines).map_err(|source| writing(&file, source))?;
        // git runs its editor as `sh -c '<editor> "$@"'` with the file it
        // wants written: a function that copies the plan over it, in words
        // every `sh` has built in.
        let editor = format!(
            "f() {{ while IFS= read -r line || [ -n \"$line\" ]; do printf '%s\\n' \"$line\"; done < {} > \"$1\"; }}; f",
            quoted(&file)
        );
        let base = match &plan.onto {
            Some(onto) => onto.id.as_str(),
            None => "--root",
        };
        self.step_in(
            &[
                "rebase",
                "--interactive",
                "--autostash",
                "--empty=drop",
                base,
            ],
            &[("GIT_SEQUENCE_EDITOR", &editor)],
        )
    }

    /// The lines of git's todo list for `todo`, once it is known to name
    /// every commit of `plan` once; the messages of the commits it rewords
    /// are written into `kept`.
    fn todo_lines(
        &self,
        plan: &RebasePlan,
        todo: &[RebaseTodo],
        kept: &Path,
    ) -> Result<String, Error> {
        let known: HashSet<&str> = plan.steps.iter().map(|s| s.commit.id.as_str()).collect();
        let mut seen = HashSet::new();
        let mut lines = String::new();
        // Whether a commit is replayed before this one, to fold it into.
        let mut any = false;
        for (number, each) in todo.iter().enumerate() {
            let id = self.repo.resolve(&each.commit)?;
            let hex = id.to_string();
            if !known.contains(hex.as_str()) {
                return Err(Error::Operation(format!(
                    "{} is not one of the commits this rebase replays",
                    short(&id)
                )));
            }
            if !seen.insert(hex.clone()) {
                return Err(Error::Operation(format!(
                    "{} is named twice: a commit is replayed once",
                    short(&id)
                )));
            }
            match &each.action {
                RebaseAction::Pick => {
                    lines.push_str(&format!("pick {hex}\n"));
                    any = true;
                }
                RebaseAction::Reword { message } => {
                    let message = message.trim();
                    if message.is_empty() {
                        return Err(Error::Operation(format!(
                            "{} is given no message: a commit needs one",
                            short(&id)
                        )));
                    }
                    let file = kept.join(format!("message-{number}"));
                    std::fs::write(&file, format!("{message}\n"))
                        .map_err(|source| writing(&file, source))?;
                    lines.push_str(&format!(
                        "pick {hex}\nexec git commit --amend --quiet --file={}\n",
                        quoted(&file)
                    ));
                    any = true;
                }
                RebaseAction::Squash | RebaseAction::Fixup => {
                    if !any {
                        return Err(Error::Operation(format!(
                            "{} has no commit before it to be folded into",
                            short(&id)
                        )));
                    }
                    let word = if each.action == RebaseAction::Squash {
                        "squash"
                    } else {
                        "fixup"
                    };
                    lines.push_str(&format!("{word} {hex}\n"));
                }
                RebaseAction::Drop => lines.push_str(&format!("drop {hex}\n")),
            }
        }
        let left: Vec<&str> = plan
            .steps
            .iter()
            .map(|step| step.commit.id.as_str())
            .filter(|id| !seen.contains(*id))
            .collect();
        if let Some(first) = left.first() {
            let more = match left.len() - 1 {
                0 => String::new(),
                more => format!(" and {more} more"),
            };
            return Err(Error::Operation(format!(
                "the plan leaves out {}{more}: a commit to leave out is named with drop",
                &first[..7]
            )));
        }
        Ok(lines)
    }

    /// Makes one commit of `revision` and every commit after it up to
    /// `HEAD`, in the place of `revision`, with `message` - or, without one,
    /// their messages one after another, oldest first. The author stays
    /// `revision`'s, and the hooks run. Nothing can conflict: the commit
    /// holds what `HEAD` holds.
    pub fn squash(&self, revision: &str, message: Option<&str>) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let (plan, target) = self.rewritten_from(revision)?;
        if plan.steps.len() < 2 {
            return Err(Error::Operation(format!(
                "{} is the commit HEAD is on: there is nothing after it to squash into it",
                short(&target)
            )));
        }
        let head = plan.steps[plan.steps.len() - 1].commit.id.clone();
        if self.index_tree()? != self.tree_of(&head)? {
            return Err(Error::Operation(
                "something is staged: it would go into the squashed commit; commit it or unstage it first"
                    .to_owned(),
            ));
        }
        let message = match message.map(str::trim).filter(|m| !m.is_empty()) {
            Some(message) => message.to_owned(),
            None => plan
                .steps
                .iter()
                .map(|step| step.message.trim())
                .collect::<Vec<_>>()
                .join("\n\n"),
        };
        let target = target.to_string();
        self.stepping(|| {
            self.change_index(&["reset", "--soft", &target], None)?;
            let made = self.change_index(
                &["commit", "--amend", "--quiet", "--file=-"],
                Some(message.as_bytes()),
            );
            if let Err(error) = made {
                // A hook that refused the message leaves the branch where
                // it was.
                let _ = self.change_index(&["reset", "--soft", &head], None);
                return Err(error);
            }
            Ok(())
        })
    }

    /// Puts what is staged into the commit `target`, as if it had been made
    /// with it: the commits after it are replayed on the result. Changes not
    /// staged stay in the working tree. A change the commits after `target`
    /// do not replay over cleanly is refused, and everything is as it was.
    pub fn absorb(&self, target: &str) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let tree = self.index_tree()?;
        self.fold(target, &tree, None)
    }

    /// Puts the lines `picks` name - of the hunks [`Repository::diff`](crate::Repository::diff)
    /// gives for `file` between the index and the working tree - into the
    /// commit `target`, and nothing else: what is staged stays out of it.
    pub fn absorb_lines(
        &self,
        file: &FileChange,
        picks: &[Picked],
        target: &str,
    ) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let Some(patch) = self.repo.lines_patch(file, picks)? else {
            return Err(Error::Operation(
                "no line is named: there is nothing to absorb".to_owned(),
            ));
        };
        let Some(head) = self.repo.head()?.commit else {
            return Err(Error::Operation(
                "there is no commit here yet: nothing to absorb into".to_owned(),
            ));
        };
        // What `HEAD` holds with these lines and nothing else, worked out
        // in an index of its own: the real one keeps what is staged.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let index = self.repo.git_dir().join(format!(
            "furca-absorb-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let shown = index.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", shown.as_str())];
        let tree = self
            .change_index_in(&["read-tree", &head], None, &env)
            .and_then(|_| {
                self.change_index_in(
                    &["apply", "--cached", "--whitespace=nowarn", "-"],
                    Some(&patch),
                    &env,
                )
            })
            .and_then(|_| self.change_index_in(&["write-tree"], None, &env));
        let _ = std::fs::remove_file(&index);
        let tree = tree.map_err(|error| match error {
            Error::Git { stderr, .. } => Error::Operation(format!(
                "these lines do not fit {} as HEAD has it - other lines of it are staged: {stderr}",
                file.path
            )),
            other => other,
        })?;
        self.fold(target, tree.trim(), Some(&patch))
    }

    /// Folds the difference between `HEAD` and `tree` into the commit
    /// `target`: a commit that holds `tree` is made on `HEAD`, marked as a
    /// fixup of `target`, and git's autosquash replays it right after
    /// `target`, as one commit with it. `lines` is the patch that takes the
    /// index from where it stands to holding the change as well, when it does
    /// not hold it yet.
    fn fold(&self, target: &str, tree: &str, lines: Option<&[u8]>) -> Result<Stepped, Error> {
        let (plan, target) = self.rewritten_from(target)?;
        let head = plan.steps[plan.steps.len() - 1].commit.id.clone();
        if tree == self.tree_of(&head)? {
            return Err(Error::Operation(
                "nothing is staged: there is nothing to absorb".to_owned(),
            ));
        }
        // By its id: a subject two commits share would name the first.
        let fixup = self.run_with(
            &["commit-tree", tree, "-p", &head],
            Some(format!("fixup! {target}\n").as_bytes()),
        )?;
        let fixup = fixup.trim();
        let base = match &plan.onto {
            Some(onto) => onto.id.as_str(),
            None => "--root",
        };
        // git's autostash brings every change back unstaged: what the index
        // held is read back into it from the tree it made, on either way out.
        let was = self.index_tree()?;
        let put_back = || {
            if self.repo.inner.state().map(operation) == Some(Operation::Rebase) {
                let _ = self.change_index(&["rebase", "--abort"], None);
            }
            let _ = self.change_index(&["reset", "--soft", &head], None);
            let _ = self.change_index(&["read-tree", &was], None);
        };
        self.stepping(|| {
            if let Some(patch) = lines {
                self.apply_to_index(patch)?;
            }
            let staged = self.index_tree()?;
            let replayed = self
                .change_index(&["reset", "--soft", fixup], None)
                .and_then(|_| {
                    self.change_index_in(
                        &[
                            "rebase",
                            "--interactive",
                            "--autosquash",
                            "--autostash",
                            base,
                        ],
                        None,
                        // The plan git writes is taken as it is.
                        &[("GIT_SEQUENCE_EDITOR", ":")],
                    )
                });
            let halfway = self.repo.inner.state().is_some();
            match replayed {
                Ok(_) if !halfway => self.change_index(&["read-tree", &staged], None).map(drop),
                done => {
                    // Read before the rebase is aborted and they are gone.
                    let paths: Vec<String> = self
                        .repo
                        .conflicts()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|conflict| conflict.path)
                        .collect();
                    put_back();
                    let said = match done {
                        Err(Error::Git { stderr, .. }) => stderr,
                        Err(other) => return Err(other),
                        Ok(_) => String::new(),
                    };
                    Err(Error::Operation(not_absorbed(
                        &short(&target),
                        &paths,
                        &said,
                    )))
                }
            }
        })
    }

    /// The plan that replays `revision` and every commit after it, once
    /// `revision` is known to be on the line of commits `HEAD` came by, with
    /// no merge from it on.
    fn rewritten_from(&self, revision: &str) -> Result<(RebasePlan, gix::ObjectId), Error> {
        let target = self.repo.resolve(revision)?;
        let commit = self.repo.read_commit(target)?;
        if commit.parents.len() > 1 {
            return Err(Error::Operation(format!(
                "{} is a merge: it is not rewritten here",
                short(&target)
            )));
        }
        let plan = self
            .repo
            .rebase_plan(commit.parents.first().map(String::as_str))?;
        if plan.steps.first().map(|step| step.commit.id.as_str()) != Some(commit.id.as_str()) {
            return Err(Error::Operation(format!(
                "{} is not a commit of the line HEAD came by: only those are rewritten",
                short(&target)
            )));
        }
        Ok((plan, target))
    }

    /// The tree the index would commit, written by git.
    fn index_tree(&self) -> Result<String, Error> {
        Ok(self.change_index(&["write-tree"], None)?.trim().to_owned())
    }

    /// The tree the commit `id` holds.
    fn tree_of(&self, id: &str) -> Result<String, Error> {
        let failed = |source: crate::error::Source| Error::Commit {
            id: id.to_owned(),
            source,
        };
        let object = gix::ObjectId::from_hex(id.as_bytes()).map_err(|e| failed(e.into()))?;
        let commit = self
            .repo
            .inner
            .find_commit(object)
            .map_err(|e| failed(e.into()))?;
        Ok(commit.tree_id().map_err(|e| failed(e.into()))?.to_string())
    }
}

/// Why `target` did not take a change: the paths the replay stopped at, or
/// else what git said - without the advice it gives for a rebase left
/// halfway, which this one is not.
fn not_absorbed(target: &str, paths: &[String], said: &str) -> String {
    let why = if paths.is_empty() {
        let words: Vec<&str> = said
            .lines()
            .filter(|line| !line.starts_with("hint:"))
            .collect();
        match words.first() {
            Some(_) => format!(
                "git stopped replaying the commits:
{}",
                words.join(
                    "
"
                )
            ),
            None => "git stopped replaying the commits".to_owned(),
        }
    } else {
        format!(
            "replaying it there stopped at a conflict in {} - that commit, or one after it, changes the same lines",
            paths.join(", ")
        )
    };
    format!("{target} does not take this change: {why}. Nothing was changed.")
}

/// `path` as one word of a line of shell, whatever its characters.
fn quoted(path: &Path) -> String {
    format!("'{}'", for_a_hook(path).replace('\'', r"'\''"))
}

fn writing(path: &Path, source: std::io::Error) -> Error {
    Error::Write {
        path: PathBuf::from(path).display().to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::{not_absorbed, quoted};
    use std::path::Path;

    #[test]
    fn a_change_not_absorbed_names_the_files_and_none_of_gits_advice() {
        assert_eq!(
            not_absorbed("1a2b3c4", &["src/a.rs".to_owned()], "hint: resolve"),
            "1a2b3c4 does not take this change: replaying it there stopped at a conflict in src/a.rs - that commit, or one after it, changes the same lines. Nothing was changed."
        );
        assert_eq!(
            not_absorbed(
                "1a2b3c4",
                &[],
                "error: no
hint: try this"
            ),
            "1a2b3c4 does not take this change: git stopped replaying the commits:
error: no. Nothing was changed."
        );
    }

    #[test]
    fn a_path_is_one_word_of_shell_whatever_it_holds() {
        assert_eq!(quoted(Path::new("/work/.git/todo")), "'/work/.git/todo'");
        assert_eq!(
            quoted(Path::new("/it's here/.git/todo")),
            r"'/it'\''s here/.git/todo'"
        );
    }
}
