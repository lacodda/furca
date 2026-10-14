//! The publish workflow's trigger: whether pushing the release's tag starts
//! it, and - when the trigger is held - putting it back.
//!
//! The line holds the tag trigger of a new repository's publish workflow
//! until its first publish by hand, because a tag cannot create a package.
//! Under the engine the hold protects nothing: a run never tags before every
//! package is in its registry. So the plan reads the workflow's `on:` as
//! GitHub and Forgejo do, and the bump puts a held trigger back.
//!
//! The reading is a YAML parser's, never a search for words: `tags:` in a
//! comment or under `branches` is not a trigger. The writing is a text edit
//! of the `on:` block alone, so every comment and blank line stays, and it
//! is read back before it counts.

use yaml_rust2::{Yaml, YamlLoader};

/// What `on: push: tags:` the engine puts back.
pub(crate) const PUT_BACK: &str = "tags: ['v*']";

/// Whether pushing a tag starts a workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Starts {
    /// The tag starts it.
    Yes,
    /// A push of tags starts it, but its filters leave this tag out.
    Filtered(Vec<String>),
    /// Nothing pushed starts it: the trigger is held, and can be put back.
    Held(Vec<String>),
    /// Pushes of branches start it and tags do not: putting a tag trigger
    /// in would change what the branches do, so it is left to a person.
    BranchesOnly,
}

/// Reads the workflow `text` for whether pushing `tag` starts it. `Err`
/// says why the text is not a workflow the engine can read.
pub(crate) fn starts(text: &str, tag: &str) -> Result<Starts, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| format!("it is not YAML: {e}"))?;
    let on = docs
        .first()
        .and_then(Yaml::as_hash)
        .and_then(|doc| doc.get(&Yaml::String("on".to_owned())))
        .ok_or("it has no `on:`")?;
    let events = |list: &[Yaml]| -> Vec<String> {
        list.iter()
            .filter_map(|e| e.as_str().map(str::to_owned))
            .collect()
    };
    match on {
        Yaml::String(event) if event == "push" => Ok(Starts::Yes),
        Yaml::String(event) => Ok(Starts::Held(vec![event.clone()])),
        Yaml::Array(list) => {
            let list = events(list);
            Ok(if list.iter().any(|e| e == "push") {
                Starts::Yes
            } else {
                Starts::Held(list)
            })
        }
        Yaml::Hash(map) => {
            let names: Vec<String> = map
                .keys()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect();
            match map.get(&Yaml::String("push".to_owned())) {
                None => Ok(Starts::Held(names)),
                Some(Yaml::Null) => Ok(Starts::Yes),
                Some(Yaml::Hash(push)) => {
                    let list = |key: &str| -> Option<Vec<String>> {
                        match push.get(&Yaml::String(key.to_owned()))? {
                            Yaml::String(one) => Some(vec![one.clone()]),
                            Yaml::Array(many) => Some(events(many)),
                            _ => Some(Vec::new()),
                        }
                    };
                    if let Some(tags) = list("tags") {
                        return Ok(if included(&tags, tag) {
                            Starts::Yes
                        } else {
                            Starts::Filtered(tags)
                        });
                    }
                    if let Some(ignored) = list("tags-ignore") {
                        return Ok(if ignored.iter().any(|p| matches(p, tag)) {
                            Starts::Filtered(ignored.iter().map(|p| format!("!{p}")).collect())
                        } else {
                            Starts::Yes
                        });
                    }
                    // A filter of branches alone keeps tags out; a filter of
                    // paths is not read for a pushed tag.
                    if list("branches").is_some() || list("branches-ignore").is_some() {
                        Ok(Starts::BranchesOnly)
                    } else {
                        Ok(Starts::Yes)
                    }
                }
                Some(_) => Err("its `on: push:` is neither empty nor a map".to_owned()),
            }
        }
        _ => Err("its `on:` is neither an event, a list nor a map".to_owned()),
    }
}

/// Puts `push: tags: ['v*']` into the `on:` of a workflow whose trigger is
/// held, keeping every other line as it was. Handles `on: event`,
/// `on: [a, b]` and a block map under `on:`; `Err` for any other shape.
pub(crate) fn put_back(text: &str) -> Result<String, String> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at = lines
        .iter()
        .position(|line| on_key(line).is_some())
        .ok_or("no `on:` at the start of a line")?;
    let rest = on_key(lines[at]).unwrap_or_default();
    let value = rest.split(" #").next().unwrap_or_default().trim();

    let mut out: Vec<String> = lines[..at].iter().map(|l| (*l).to_owned()).collect();
    if value.is_empty() {
        // A block map: the new key goes first, indented like its siblings.
        let indent = lines[at + 1..]
            .iter()
            .map(|l| l.trim_end_matches(['\r', '\n']))
            .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(|l| &l[..l.len() - l.trim_start().len()])
            .filter(|i| !i.is_empty())
            .ok_or("`on:` is followed by nothing indented")?;
        out.push(lines[at].to_owned());
        out.push(format!("{indent}push:{newline}"));
        out.push(format!("{indent}{indent}{PUT_BACK}{newline}"));
    } else {
        let events: Vec<&str> = match value.strip_prefix('[') {
            Some(list) => list
                .strip_suffix(']')
                .ok_or("an `on: [...]` list that does not close on its line")?
                .split(',')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .collect(),
            None => vec![value],
        };
        if events
            .iter()
            .any(|e| !e.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        {
            return Err(format!("`on: {value}` is not a list of plain event names"));
        }
        let head = &lines[at][..lines[at].find(':').unwrap_or(0) + 1];
        out.push(format!("{head}{newline}"));
        out.push(format!("  push:{newline}"));
        out.push(format!("    {PUT_BACK}{newline}"));
        for event in events {
            out.push(format!("  {event}:{newline}"));
        }
    }
    out.extend(lines[at + 1..].iter().map(|l| (*l).to_owned()));
    Ok(out.concat())
}

/// What follows `on:` when `line` is the top-level `on` key.
fn on_key(line: &str) -> Option<&str> {
    ["on:", "\"on\":", "'on':"]
        .iter()
        .find_map(|key| line.strip_prefix(key))
}

/// Whether the filter `patterns` takes `name` in: they are read in order, a
/// pattern takes it in, a `!pattern` after it takes it out again.
fn included(patterns: &[String], name: &str) -> bool {
    let mut taken = false;
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(negated) => {
                if matches(negated, name) {
                    taken = false;
                }
            }
            None => {
                if matches(pattern, name) {
                    taken = true;
                }
            }
        }
    }
    taken
}

/// One piece of a filter pattern.
#[derive(Debug, Clone)]
enum Atom {
    Char(char),
    /// `[...]`: the ranges it accepts.
    Class(Vec<(char, char)>),
    /// `*` (no `/`) or `**` (anything).
    Any {
        slash: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum Count {
    One,
    /// `?` after an atom: zero or one of it.
    Maybe,
    /// `+` after an atom: one or more of it.
    Many,
}

/// Whether `name` matches a GitHub filter pattern: `*` is any run without
/// `/`, `**` any run at all, `?` and `+` make the atom before them optional
/// or repeated, `[a-z]` is a class, and `\` takes the next character as it
/// is.
pub(crate) fn matches(pattern: &str, name: &str) -> bool {
    let mut atoms: Vec<(Atom, Count)> = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        let atom = match c {
            '*' => {
                let slash = chars.peek() == Some(&'*');
                if slash {
                    chars.next();
                }
                Atom::Any { slash }
            }
            '?' | '+' => {
                if let Some(last) = atoms.last_mut() {
                    last.1 = if c == '?' { Count::Maybe } else { Count::Many };
                    continue;
                }
                Atom::Char(c)
            }
            '[' => {
                let mut ranges = Vec::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    if chars.peek() == Some(&'-') {
                        chars.next();
                        match chars.next() {
                            Some(']') | None => {
                                ranges.push((c, c));
                                ranges.push(('-', '-'));
                                closed = true;
                                break;
                            }
                            Some(end) => ranges.push((c, end)),
                        }
                    } else {
                        ranges.push((c, c));
                    }
                }
                if !closed {
                    return false;
                }
                Atom::Class(ranges)
            }
            '\\' => Atom::Char(chars.next().unwrap_or('\\')),
            c => Atom::Char(c),
        };
        atoms.push((atom, Count::One));
    }
    let name: Vec<char> = name.chars().collect();
    walk(&atoms, &name)
}

fn walk(atoms: &[(Atom, Count)], name: &[char]) -> bool {
    let Some(((atom, count), rest)) = atoms.split_first() else {
        return name.is_empty();
    };
    if let Atom::Any { slash } = atom {
        // The longest run first; any shorter one may let the rest match.
        let longest = name
            .iter()
            .position(|c| *c == '/' && !slash)
            .unwrap_or(name.len());
        return (0..=longest).rev().any(|n| walk(rest, &name[n..]));
    }
    let one = |c: &char| match atom {
        Atom::Char(want) => c == want,
        Atom::Class(ranges) => ranges.iter().any(|(a, b)| (a..=b).contains(&c)),
        Atom::Any { .. } => true,
    };
    let run = name.iter().take_while(|c| one(c)).count();
    match count {
        Count::One => run >= 1 && walk(rest, &name[1..]),
        Count::Maybe => (run >= 1 && walk(rest, &name[1..])) || walk(rest, name),
        Count::Many => (1..=run).rev().any(|n| walk(rest, &name[n..])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_patterns_match_as_github_reads_them() {
        for (pattern, name, expected) in [
            ("v*", "v0.5.0", true),
            ("v*", "release-1", false),
            ("v*", "v1/x", false),
            ("**", "v1/x", true),
            ("v[0-9]+.[0-9]+.[0-9]+", "v0.12.3", true),
            ("v[0-9]+.[0-9]+.[0-9]+", "v0.12", false),
            ("v1.?0", "v1.0", true),
            ("v1.?0", "v10", true),
            ("v1.?0", "v1x0", false),
            ("v[12].*", "v2.3.0", true),
            ("v[12].*", "v3.0.0", false),
            ("\\*", "*", true),
            ("v[0-9", "v1", false),
        ] {
            assert_eq!(matches(pattern, name), expected, "{pattern} on {name}");
        }
        let filter = ["v*".to_owned(), "!v*-rc*".to_owned()];
        assert!(included(&filter, "v1.0.0"));
        assert!(!included(&filter, "v1.0.0-rc.1"));
    }

    #[test]
    fn a_workflow_says_whether_a_tag_starts_it() {
        let read = |text: &str| starts(text, "v0.5.0").expect("reads");
        assert_eq!(read("on: push\n"), Starts::Yes);
        assert_eq!(read("on: [push, workflow_dispatch]\n"), Starts::Yes);
        assert_eq!(read("on:\n  push:\n    tags: ['v*']\n"), Starts::Yes);
        assert_eq!(read("on:\n  push:\n    tags:\n      - 'v*'\n"), Starts::Yes);
        assert_eq!(
            read("on:\n  push:\n    tags: ['release-*']\n"),
            Starts::Filtered(vec!["release-*".to_owned()])
        );
        assert_eq!(read("on:\n  push:\n    tags-ignore: ['x*']\n"), Starts::Yes);
        assert_eq!(
            read("on:\n  push:\n    branches: [main]\n"),
            Starts::BranchesOnly
        );
        assert_eq!(
            read("on:\n  push:\n    paths: ['src/**']\n"),
            Starts::Yes,
            "a paths filter is not read for tags"
        );
        assert_eq!(
            read("on: workflow_dispatch\n"),
            Starts::Held(vec!["workflow_dispatch".to_owned()])
        );
        assert_eq!(
            read(
                "on:\n  # tags: ['v*'] held until the first publish\n  workflow_dispatch:\n    inputs:\n      tag:\n        required: true\n"
            ),
            Starts::Held(vec!["workflow_dispatch".to_owned()]),
            "a trigger in a comment is no trigger"
        );
        assert!(starts("name: x\n", "v1.0.0").is_err());
        assert!(starts("on: [unclosed\n", "v1.0.0").is_err());
    }

    #[test]
    fn a_held_trigger_is_put_back_and_the_rest_kept() {
        let block = "name: Publish\n\n# Held until the first publish.\non:\n  workflow_dispatch:\n    inputs:\n      tag:\n        required: true\n\njobs: {}\n";
        let back = put_back(block).expect("puts back");
        assert_eq!(
            back,
            "name: Publish\n\n# Held until the first publish.\non:\n  push:\n    tags: ['v*']\n  workflow_dispatch:\n    inputs:\n      tag:\n        required: true\n\njobs: {}\n"
        );
        assert_eq!(starts(&back, "v0.5.0"), Ok(Starts::Yes));

        let scalar = "on: workflow_dispatch # by hand only\r\njobs: {}\r\n";
        let back = put_back(scalar).expect("puts back");
        assert_eq!(
            back,
            "on:\r\n  push:\r\n    tags: ['v*']\r\n  workflow_dispatch:\r\njobs: {}\r\n"
        );
        assert_eq!(starts(&back, "v0.5.0"), Ok(Starts::Yes));

        let list = "on: [workflow_dispatch, release]\njobs: {}\n";
        let back = put_back(list).expect("puts back");
        assert_eq!(starts(&back, "v0.5.0"), Ok(Starts::Yes));
        assert!(back.contains("  release:\n"));

        assert!(put_back("on: {workflow_dispatch: {}}\n").is_err());
        assert!(put_back("on:\n").is_err());
    }
}
