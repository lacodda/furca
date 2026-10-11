//! The bump: every version the plan found, rewritten to the new number with
//! each file's own formatting kept - comments, key order, indentation, line
//! endings. The value changes and nothing around it does.
//!
//! The bump rewrites exactly the places the plan listed and refuses a file
//! that no longer holds the old number where the plan saw it: the file
//! changed between the plan and the run, and guessing where the number went
//! is how a release edits the wrong line.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::manifest::{Field, Place};
use crate::texts::Mention;

/// A file a run rewrote: its text before and after, so that a run stopped
/// before its commit can put it back - and can tell when someone else has
/// changed it since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Touched {
    /// The file, relative to the root, with `/` separators.
    pub path: String,
    pub before: String,
    pub after: String,
}

/// Rewrites `fields` and `mentions` to `to` and returns the files it
/// changed, in path order. Every new text is made before the first one is
/// written: a file the bump refuses leaves every file as it was.
pub(crate) fn apply(
    root: &Path,
    fields: &[Field],
    mentions: &[Mention],
    to: &str,
) -> Result<Vec<Touched>, Error> {
    let mut files: BTreeMap<&str, (Vec<&Field>, Vec<&Mention>)> = BTreeMap::new();
    for field in fields {
        files.entry(&field.path).or_default().0.push(field);
    }
    for mention in mentions {
        files.entry(&mention.path).or_default().1.push(mention);
    }

    let mut touched = Vec::new();
    for (path, (fields, mentions)) in files {
        let full = root.join(path);
        let before = std::fs::read_to_string(&full).map_err(|source| Error::Read {
            path: full.clone(),
            source,
        })?;
        let refuse = |why: String| Error::Bump {
            path: path.to_owned(),
            why,
        };
        let mut after = before.clone();
        if !fields.is_empty() {
            after = rewrite_fields(&after, &fields, to).map_err(refuse)?;
        }
        if !mentions.is_empty() {
            after = rewrite_mentions(&after, &mentions, to).map_err(refuse)?;
        }
        if after != before {
            touched.push(Touched {
                path: path.to_owned(),
                before,
                after,
            });
        }
    }
    write(root, &touched)?;
    Ok(touched)
}

/// Writes each file's `after`; if one cannot be written, the ones already
/// written are put back.
pub(crate) fn write(root: &Path, touched: &[Touched]) -> Result<(), Error> {
    for (index, file) in touched.iter().enumerate() {
        let full = root.join(&file.path);
        if let Err(source) = std::fs::write(&full, &file.after) {
            for done in &touched[..index] {
                let _ = std::fs::write(root.join(&done.path), &done.before);
            }
            return Err(Error::Write { path: full, source });
        }
    }
    Ok(())
}

/// Puts back what a run wrote: each file that still holds the run's text
/// gets its old text again. Returns the files someone has changed since,
/// which are left as they are.
pub(crate) fn take_back(root: &Path, touched: &[Touched]) -> Result<Vec<String>, Error> {
    let mut changed = Vec::new();
    for file in touched {
        let full = root.join(&file.path);
        let now = std::fs::read_to_string(&full).map_err(|source| Error::Read {
            path: full.clone(),
            source,
        })?;
        if now == file.after {
            std::fs::write(&full, &file.before).map_err(|source| Error::Write {
                path: full.clone(),
                source,
            })?;
        } else if now != file.before {
            changed.push(file.path.clone());
        }
    }
    Ok(changed)
}

/// The manifest fields of one file, all of one kind.
fn rewrite_fields(text: &str, fields: &[&Field], to: &str) -> Result<String, String> {
    if fields.iter().all(|f| f.place == Place::Json) {
        let from = &fields[0].version;
        return set_json_version(text, from, to);
    }
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| format!("is not valid TOML: {}", e.message()))?;
    // toml_edit keeps a document byte for byte; a file it would not give
    // back unchanged is one the bump would reformat, so it is left alone.
    if doc.to_string() != text {
        return Err(
            "would not be written back as it is: furca keeps a manifest's formatting or leaves it alone"
                .to_owned(),
        );
    }
    for field in fields {
        match &field.place {
            Place::Toml(keys) => set_toml(doc.as_item_mut(), keys, &field.version, to)?,
            Place::Lock(name) => set_lock(&mut doc, name, &field.version, to)?,
            Place::Json => return Err("mixes JSON and TOML fields".to_owned()),
        }
    }
    Ok(doc.to_string())
}

fn set_toml(
    item: &mut toml_edit::Item,
    keys: &[String],
    from: &str,
    to: &str,
) -> Result<(), String> {
    let mut item = item;
    for key in keys {
        item = item
            .get_mut(key.as_str())
            .ok_or_else(|| format!("has no `{}` any more", keys.join(".")))?;
    }
    let value = item
        .as_value_mut()
        .ok_or_else(|| format!("`{}` is not a value", keys.join(".")))?;
    set_string(value, from, to).map_err(|why| format!("`{}` {why}", keys.join(".")))
}

fn set_lock(
    doc: &mut toml_edit::DocumentMut,
    name: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let packages = doc
        .get_mut("package")
        .and_then(|p| p.as_array_of_tables_mut())
        .ok_or_else(|| "has no [[package]] entries".to_owned())?;
    let entry = packages
        .iter_mut()
        .find(|t| t.get("name").and_then(|v| v.as_str()) == Some(name) && t.get("source").is_none())
        .ok_or_else(|| format!("has no entry for `{name}` any more"))?;
    let value = entry
        .get_mut("version")
        .and_then(|v| v.as_value_mut())
        .ok_or_else(|| format!("the entry for `{name}` has no version"))?;
    set_string(value, from, to).map_err(|why| format!("the entry for `{name}` {why}"))
}

/// Replaces a string value that says `from`, keeping what surrounds it.
fn set_string(value: &mut toml_edit::Value, from: &str, to: &str) -> Result<(), String> {
    match value.as_str() {
        Some(now) if now == from => {
            let decor = value.decor().clone();
            let mut new = toml_edit::Value::from(to);
            *new.decor_mut() = decor;
            *value = new;
            Ok(())
        }
        Some(now) => Err(format!("says {now}, not {from}")),
        None => Err("is not a string".to_owned()),
    }
}

/// Replaces the top-level `"version"` of a JSON document, touching nothing
/// but the characters between its quotes.
fn set_json_version(text: &str, from: &str, to: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    // The container at each depth: `true` for an object.
    let mut stack: Vec<bool> = Vec::new();
    let mut expecting_key = false;
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                let end = string_end(bytes, at).ok_or("has a string that never closes")?;
                let is_key = stack.len() == 1 && expecting_key;
                if is_key && &text[at + 1..end] == "version" {
                    let mut value = skip_space(bytes, end + 1);
                    if bytes.get(value) != Some(&b':') {
                        return Err("has a \"version\" key without a value".to_owned());
                    }
                    value = skip_space(bytes, value + 1);
                    if bytes.get(value) != Some(&b'"') {
                        return Err("has a \"version\" that is not a string".to_owned());
                    }
                    let close = string_end(bytes, value).ok_or("has a string that never closes")?;
                    let now = &text[value + 1..close];
                    if now != from {
                        return Err(format!("says \"version\": \"{now}\", not {from}"));
                    }
                    let out = format!("{}{to}{}", &text[..value + 1], &text[close..]);
                    let check: serde_json::Value = serde_json::from_str(&out)
                        .map_err(|e| format!("would not be valid JSON after the bump: {e}"))?;
                    if check.get("version").and_then(|v| v.as_str()) != Some(to) {
                        return Err("would not carry the new version after the bump".to_owned());
                    }
                    return Ok(out);
                }
                expecting_key = false;
                at = end + 1;
                continue;
            }
            b'{' => {
                stack.push(true);
                expecting_key = stack.len() == 1;
            }
            b'[' => stack.push(false),
            b'}' | b']' => {
                stack.pop();
            }
            b',' => expecting_key = stack.len() == 1 && stack.last() == Some(&true),
            b':' => expecting_key = false,
            _ => {}
        }
        at += 1;
    }
    Err("has no top-level \"version\"".to_owned())
}

/// The index of the quote that closes the string opening at `open`.
fn string_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut at = open + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return Some(at),
            _ => at += 1,
        }
    }
    None
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| b.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

/// Rewrites each mention where the scan found it - its line and its column -
/// and nowhere else.
fn rewrite_mentions(text: &str, mentions: &[&Mention], to: &str) -> Result<String, String> {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
    let mut sorted: Vec<&&Mention> = mentions.iter().collect();
    // Right to left within a line, so each column still points where the
    // scan saw it.
    sorted.sort_by_key(|m| (m.line, std::cmp::Reverse(m.column)));
    for mention in sorted {
        let line = lines
            .get_mut(mention.line - 1)
            .ok_or_else(|| format!("has no line {} any more", mention.line))?;
        let end = mention.column + mention.text.len();
        if line.get(mention.column..end) != Some(mention.text.as_str()) {
            return Err(format!(
                "line {} no longer says `{}` where the plan saw it",
                mention.line, mention.text
            ));
        }
        let prefix = &mention.text[..mention.text.len() - mention.version.len()];
        line.replace_range(mention.column..end, &format!("{prefix}{to}"));
    }
    Ok(lines.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_field(path: &str, keys: &[&str], version: &str) -> Field {
        Field {
            path: path.to_owned(),
            key: keys.join("."),
            version: version.to_owned(),
            place: Place::Toml(keys.iter().map(|k| (*k).to_owned()).collect()),
        }
    }

    #[test]
    fn a_manifest_keeps_its_comments_and_layout() {
        let text = "[workspace.package]\n# The one number.\nversion = \"0.3.0\" # bumped\n\n[dependencies]\ncore = { path = \"../core\", version = \"0.3.0\" }\n\"dotted.name\" = { path = \"x\", version = \"0.3.0\" }\n";
        let fields = [
            toml_field("Cargo.toml", &["workspace", "package", "version"], "0.3.0"),
            toml_field("Cargo.toml", &["dependencies", "core", "version"], "0.3.0"),
            toml_field(
                "Cargo.toml",
                &["dependencies", "dotted.name", "version"],
                "0.3.0",
            ),
        ];
        let fields: Vec<&Field> = fields.iter().collect();
        assert_eq!(
            rewrite_fields(text, &fields, "0.4.0").expect("rewrites"),
            text.replace("0.3.0", "0.4.0")
        );
    }

    #[test]
    fn a_manifest_that_moved_on_is_refused() {
        let field = toml_field("Cargo.toml", &["package", "version"], "0.3.0");
        let error = rewrite_fields("[package]\nversion = \"0.3.1\"\n", &[&field], "0.4.0")
            .expect_err("the plan saw 0.3.0");
        assert!(error.contains("says 0.3.1"), "{error}");
    }

    #[test]
    fn a_lockfile_moves_only_the_workspace_entry() {
        let text = "version = 4\n\n[[package]]\nname = \"core\"\nversion = \"0.3.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"core\"\nversion = \"0.3.0\"\ndependencies = [\n \"serde\",\n]\n";
        let field = Field {
            path: "Cargo.lock".to_owned(),
            key: "package.core.version".to_owned(),
            version: "0.3.0".to_owned(),
            place: Place::Lock("core".to_owned()),
        };
        let out = rewrite_fields(text, &[&field], "0.4.0").expect("rewrites");
        assert_eq!(out.matches("0.3.0").count(), 1, "the registry's core stays");
        assert!(
            out.ends_with(
                "name = \"core\"\nversion = \"0.4.0\"\ndependencies = [\n \"serde\",\n]\n"
            )
        );
    }

    #[test]
    fn only_the_top_level_json_version_moves() {
        let text = "{\r\n  \"name\": \"furca\",\r\n  \"engines\": { \"version\": \"0.3.0\" },\r\n  \"list\": [\"version\", {\"version\": \"0.3.0\"}],\r\n  \"version\"  :  \"0.3.0\",\r\n  \"tail\": 1\r\n}\r\n";
        let out = set_json_version(text, "0.3.0", "0.4.0").expect("rewrites");
        assert_eq!(
            out,
            text.replace("\"version\"  :  \"0.3.0\"", "\"version\"  :  \"0.4.0\"")
        );
        assert!(set_json_version(text, "0.2.0", "0.4.0").is_err());
        assert!(set_json_version("{\"name\": \"x\"}", "0.3.0", "0.4.0").is_err());
    }

    fn mention(line: usize, column: usize, text: &str, version: &str) -> Mention {
        Mention {
            path: "README.md".to_owned(),
            line,
            column,
            text: text.to_owned(),
            version: version.to_owned(),
            history: false,
            in_table: false,
        }
    }

    #[test]
    fn mentions_move_where_they_were_seen_and_nowhere_else() {
        let text = "Get v1.2.3.\r\nurl/v1.2.3/furca-v1.2.3-x86_64.zip and furca 1.2.3\r\nkeep v1.2.3 here\r\n";
        let mentions = [
            mention(1, 4, "v1.2.3", "1.2.3"),
            mention(2, 4, "v1.2.3", "1.2.3"),
            mention(2, 17, "v1.2.3", "1.2.3"),
            mention(2, 39, "furca 1.2.3", "1.2.3"),
        ];
        let mentions: Vec<&Mention> = mentions.iter().collect();
        let out = rewrite_mentions(text, &mentions, "1.2.30").expect("rewrites");
        assert_eq!(
            out,
            "Get v1.2.30.\r\nurl/v1.2.30/furca-v1.2.30-x86_64.zip and furca 1.2.30\r\nkeep v1.2.3 here\r\n"
        );
        let stale = [mention(3, 0, "v1.2.3", "1.2.3")];
        let stale: Vec<&Mention> = stale.iter().collect();
        assert!(rewrite_mentions(text, &stale, "1.2.30").is_err());
    }
}
