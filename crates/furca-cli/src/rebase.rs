//! The commands that rewrite the commits of a branch: `rebase` - onto a
//! branch, or by a plan - `squash` and `absorb`. Each is one call into the
//! core, which reads the plan, holds a todo list to it, and has the system
//! `git` replay the commits (ADR 0024).
//!
//! A rebase that stops at a conflict answers "no" (exit 1), as a merge does:
//! `conflicts`, `resolve`, `continue` and `abort` take it from there. A
//! squash and an absorb never stop halfway: they happen, or are refused.

use std::path::Path;

use furca_core::{RebasePlan, RebaseTodo, Stepped};

use crate::merge::{at, commits, stopped_text};
use crate::{Answer, Common, emit, message_text, open, short};

/// What `rebase` was asked for beside where to land.
pub(crate) enum Asked<'a> {
    /// Replay the commits as they are.
    Replay,
    /// Say what would be replayed, and change nothing.
    Plan,
    /// Replay them as the todo list in this file says; `-` is standard input.
    Todo(&'a Path),
}

pub(crate) fn rebase(
    common: &Common,
    onto: Option<&str>,
    asked: Asked<'_>,
) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let stepped = match (asked, onto) {
        (Asked::Plan, onto) => {
            let plan = repo.rebase_plan(onto).map_err(|e| e.to_string())?;
            return emit(common.json, &plan, plan_text);
        }
        (Asked::Todo(file), onto) => {
            let text = message_text(Some(file))?;
            let todo: Vec<RebaseTodo> = serde_json::from_str(&text).map_err(|e| {
                format!(
                    "the todo list is not what `furca rebase --plan --json` leads to - a JSON list of {{\"commit\", \"action\"}}: {e}"
                )
            })?;
            repo.git().rebase_interactive(onto, &todo)
        }
        (Asked::Replay, Some(onto)) => repo.git().rebase(onto),
        (Asked::Replay, None) => {
            return Err(
                "--root replays the whole history, which only a plan changes: add --plan or --todo"
                    .to_owned(),
            );
        }
    }
    .map_err(|e| e.to_string())?;
    said(common, &stepped, rebased_text)
}

pub(crate) fn squash(
    common: &Common,
    revision: &str,
    message: Option<&str>,
) -> Result<Answer, String> {
    let stepped = open(&common.repo)?
        .git()
        .squash(revision, message)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, squashed_text)
}

pub(crate) fn absorb(common: &Common, revision: &str) -> Result<Answer, String> {
    let stepped = open(&common.repo)?
        .git()
        .absorb(revision)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, absorbed_text)
}

fn said(
    common: &Common,
    stepped: &Stepped,
    text: fn(&Stepped) -> String,
) -> Result<Answer, String> {
    emit(common.json, stepped, text)?;
    Ok(if stepped.operation.is_some() {
        Answer::No
    } else {
        Answer::Yes
    })
}

fn rebased_text(stepped: &Stepped) -> String {
    match stepped.operation {
        Some(operation) => stopped_text(stepped, operation),
        None if stepped.from == stepped.to => format!("Nothing to replay: {}.", at(stepped)),
        None => format!("Replayed {}: {}.", commits(stepped.made), at(stepped)),
    }
}

fn squashed_text(stepped: &Stepped) -> String {
    format!("Squashed into one commit: {}.", at(stepped))
}

fn absorbed_text(stepped: &Stepped) -> String {
    format!(
        "Absorbed, and {} rewritten: {}.",
        commits(stepped.made),
        at(stepped)
    )
}

/// The commits a rebase would replay, oldest first, as a todo list reads.
fn plan_text(plan: &RebasePlan) -> String {
    let onto = match &plan.onto {
        Some(onto) => format!("on {} {}", short(&onto.id), onto.subject),
        None => "from the first commit".to_owned(),
    };
    if plan.steps.is_empty() {
        return format!("Nothing to replay {onto}.");
    }
    let mut out = vec![format!(
        "{} to replay {onto}, oldest first:",
        commits(plan.steps.len())
    )];
    out.extend(plan.steps.iter().map(|step| {
        format!(
            "  {} {}{}",
            short(&step.commit.id),
            step.commit.subject,
            if step.pushed { "  (pushed)" } else { "" }
        )
    }));
    if plan.steps.iter().any(|step| step.pushed) {
        out.push(
            "Rewriting a pushed commit parts the branch from its upstream: it is pushed again with --force-with-lease."
                .to_owned(),
        );
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use furca_core::{RebaseAction, RebaseTodo};

    #[test]
    fn a_todo_list_reads_as_the_reference_shows_it() {
        let todo: Vec<RebaseTodo> = serde_json::from_str(
            r#"[
                {"commit": "1a2b3c4", "action": "pick"},
                {"commit": "5d6e7f8", "action": "reword", "message": "feat: say it better"},
                {"commit": "9a8b7c6", "action": "squash"},
                {"commit": "0f1e2d3", "action": "fixup"},
                {"commit": "4c5b6a7", "action": "drop"}
            ]"#,
        )
        .expect("reads");
        assert_eq!(todo.len(), 5);
        assert_eq!(
            todo[1].action,
            RebaseAction::Reword {
                message: "feat: say it better".to_owned()
            }
        );
        assert_eq!(todo[4].action, RebaseAction::Drop);
    }
}
