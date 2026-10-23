//! What the window remembers between runs: the open tabs, the repositories
//! opened lately, and the sessions saved under a name.
//!
//! The store is the one truth about the tabs. The window sends intents -
//! open this, close that, save these as a session - and draws the state each
//! one returns, so the order of the tabs, the recent list and the sessions
//! are decided here, where they are tested, and written to disk after every
//! change, so a crash loses nothing that was on the screen.

use std::path::{Component, Path, PathBuf, Prefix};

use serde::{Deserialize, Serialize};

/// How many repositories the recent list keeps.
pub const RECENT: usize = 12;

/// The file the state lives in, inside the app's data directory.
const FILE: &str = "state.json";

/// Where an unreadable state file is moved aside, rather than overwritten.
const BROKEN: &str = "state.broken.json";

/// Everything the window remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct WindowState {
    /// The open repositories, in the order of their tabs.
    pub tabs: Vec<Tab>,
    /// The tab in front; `None` when no tab is open.
    pub active: Option<usize>,
    /// Repositories opened lately, the most recent first.
    pub recent: Vec<Tab>,
    /// Sets of tabs saved under a name, sorted by name.
    pub sessions: Vec<Session>,
}

/// An open repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct Tab {
    /// The root of the working tree, or the git directory of a bare
    /// repository: the one path a repository is known by.
    pub path: String,
    /// What the tab says: the root's directory name.
    pub name: String,
}

/// A set of tabs saved under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct Session {
    pub name: String,
    /// The repositories it opens, in tab order.
    pub tabs: Vec<Tab>,
    /// The tab it puts in front.
    pub active: Option<usize>,
}

impl Tab {
    /// The tab for the repository rooted at `root`.
    pub fn at(root: &Path) -> Self {
        let path = root.to_string_lossy().into_owned();
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        Self { path, name }
    }
}

impl WindowState {
    /// Opens `tab` and puts it in front: the existing tab if the repository
    /// is open already, a new one after the tab in front otherwise. Either
    /// way it goes to the top of the recent list.
    pub fn open(&mut self, tab: Tab) {
        self.remember(&tab);
        if let Some(index) = self.tabs.iter().position(|open| open.path == tab.path) {
            self.active = Some(index);
            return;
        }
        let at = self.active.map_or(self.tabs.len(), |active| active + 1);
        self.tabs.insert(at, tab);
        self.active = Some(at);
    }

    /// Replaces the open tabs with `tabs`, the one at `active` in front;
    /// what a session does when it opens. A repository listed twice is
    /// opened once.
    pub fn replace(&mut self, tabs: Vec<Tab>, active: Option<usize>) {
        let wanted = active
            .and_then(|index| tabs.get(index))
            .map(|tab| tab.path.clone());
        self.tabs.clear();
        for tab in tabs {
            if !self.tabs.iter().any(|open| open.path == tab.path) {
                self.tabs.push(tab);
            }
        }
        self.active = match wanted {
            Some(path) => self.tabs.iter().position(|tab| tab.path == path),
            None => None,
        }
        .or(if self.tabs.is_empty() { None } else { Some(0) });
    }

    /// Closes the tab at `index`. The tab to its right comes to the front,
    /// or the one to its left when it was the last - the way every tabbed
    /// window behaves. An index past the end changes nothing.
    pub fn close(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(index.min(self.tabs.len() - 1)),
            other => other,
        };
    }

    /// Puts the tab at `index` in front.
    pub fn activate(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = Some(index);
        }
    }

    /// Moves the tab at `from` to `to`, keeping the same tab in front.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let front = self.active.map(|active| self.tabs[active].path.clone());
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = front.and_then(|path| self.tabs.iter().position(|tab| tab.path == path));
    }

    /// Saves the open tabs as the session `name`, replacing one of the same
    /// name. A name is trimmed; an empty one, or no tabs, saves nothing.
    pub fn save_session(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.tabs.is_empty() {
            return false;
        }
        let session = Session {
            name: name.to_owned(),
            tabs: self.tabs.clone(),
            active: self.active,
        };
        match self.sessions.iter_mut().find(|saved| saved.name == name) {
            Some(saved) => *saved = session,
            None => self.sessions.push(session),
        }
        self.sessions
            .sort_by_key(|session| session.name.to_lowercase());
        true
    }

    /// Forgets the session `name`.
    pub fn delete_session(&mut self, name: &str) {
        self.sessions.retain(|session| session.name != name);
    }

    /// The session `name`, if one is saved.
    pub fn session(&self, name: &str) -> Option<&Session> {
        self.sessions.iter().find(|session| session.name == name)
    }

    fn remember(&mut self, tab: &Tab) {
        self.recent.retain(|recent| recent.path != tab.path);
        self.recent.insert(0, tab.clone());
        self.recent.truncate(RECENT);
    }
}

/// The state in `dir`, or a fresh one when there is none yet.
///
/// A file that cannot be read is moved aside as `state.broken.json` and the
/// window starts empty: losing a list of tabs is better than a window that
/// does not open, and the file is still there to be looked at.
pub fn load(dir: &Path) -> WindowState {
    let path = dir.join(FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return WindowState::default();
    };
    match serde_json::from_str(&text) {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "furca: {} is unreadable ({error}); starting empty",
                path.display()
            );
            let _ = std::fs::rename(&path, dir.join(BROKEN));
            WindowState::default()
        }
    }
}

/// Writes `state` into `dir`: to a file beside it first, then renamed over
/// the old one, so a crash in the middle leaves the old state, not half of
/// the new one.
pub fn save(dir: &Path, state: &WindowState) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    let next = dir.join("state.json.next");
    std::fs::write(&next, text)?;
    std::fs::rename(&next, dir.join(FILE))
}

/// A repository root as the store knows it: absolute, with every link and
/// `..` resolved and the letters of each component as the disk has them, so
/// one repository reached two ways is one tab, not two.
///
/// `canonicalize` on Windows answers in the `\\?\C:\...` form, which no
/// person writes and some tools refuse; a plain drive path is given back
/// without it. A network path keeps the form it needs.
pub fn normalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    let mut components = canonical.components();
    if let Some(Component::Prefix(prefix)) = components.next()
        && let Prefix::VerbatimDisk(letter) = prefix.kind()
    {
        let mut plain = PathBuf::from(format!("{}:\\", char::from(letter)));
        for component in components {
            if !matches!(component, Component::RootDir) {
                plain.push(component.as_os_str());
            }
        }
        return Ok(plain);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(name: &str) -> Tab {
        Tab {
            path: format!("/repos/{name}"),
            name: name.to_owned(),
        }
    }

    fn names(state: &WindowState) -> Vec<&str> {
        state.tabs.iter().map(|tab| tab.name.as_str()).collect()
    }

    #[test]
    fn a_new_tab_opens_after_the_one_in_front_and_comes_to_the_front() {
        let mut state = WindowState::default();
        state.open(tab("a"));
        state.open(tab("b"));
        state.activate(0);
        state.open(tab("c"));
        assert_eq!(names(&state), ["a", "c", "b"]);
        assert_eq!(state.active, Some(1));
    }

    #[test]
    fn opening_an_open_repository_brings_its_tab_forward() {
        let mut state = WindowState::default();
        state.open(tab("a"));
        state.open(tab("b"));
        state.open(tab("a"));
        assert_eq!(names(&state), ["a", "b"]);
        assert_eq!(state.active, Some(0));
    }

    #[test]
    fn closing_the_tab_in_front_brings_its_right_neighbour_forward() {
        let mut state = WindowState::default();
        for name in ["a", "b", "c"] {
            state.open(tab(name));
        }
        state.activate(1);
        state.close(1);
        assert_eq!(names(&state), ["a", "c"]);
        assert_eq!(state.active, Some(1), "c, which was right of b");

        state.close(1);
        assert_eq!(state.active, Some(0), "the last tab closed: its left one");
        state.close(0);
        assert_eq!(state.active, None);
    }

    #[test]
    fn closing_a_tab_left_of_the_front_keeps_the_same_tab_in_front() {
        let mut state = WindowState::default();
        for name in ["a", "b", "c"] {
            state.open(tab(name));
        }
        state.close(0);
        assert_eq!(state.active.map(|i| state.tabs[i].name.as_str()), Some("c"));
    }

    #[test]
    fn moving_a_tab_keeps_the_same_tab_in_front() {
        let mut state = WindowState::default();
        for name in ["a", "b", "c"] {
            state.open(tab(name));
        }
        state.activate(0);
        state.move_tab(0, 2);
        assert_eq!(names(&state), ["b", "c", "a"]);
        assert_eq!(state.active, Some(2));
        state.move_tab(5, 0);
        assert_eq!(
            names(&state),
            ["b", "c", "a"],
            "an index past the end moves nothing"
        );

        // A tab moved past the one in front shifts it, and it stays in front.
        state.activate(0);
        state.move_tab(1, 2);
        assert_eq!(names(&state), ["b", "a", "c"]);
        assert_eq!(state.active, Some(0), "b, which did not move");
        state.move_tab(2, 0);
        assert_eq!(names(&state), ["c", "b", "a"]);
        assert_eq!(state.active, Some(1), "b, pushed right by c");
    }

    #[test]
    fn the_recent_list_is_most_recent_first_without_repeats_and_capped() {
        let mut state = WindowState::default();
        for index in 0..RECENT + 3 {
            state.open(tab(&format!("r{index}")));
        }
        state.open(tab("r5"));
        assert_eq!(state.recent.len(), RECENT);
        assert_eq!(state.recent[0].name, "r5");
        assert_eq!(state.recent.iter().filter(|t| t.name == "r5").count(), 1);
    }

    #[test]
    fn a_session_restores_its_tabs_and_the_one_in_front() {
        let mut state = WindowState::default();
        for name in ["a", "b", "c"] {
            state.open(tab(name));
        }
        state.activate(1);
        assert!(state.save_session("  work "));
        state.replace(vec![tab("x")], Some(0));

        let session = state.session("work").expect("saved, trimmed").clone();
        state.replace(session.tabs, session.active);
        assert_eq!(names(&state), ["a", "b", "c"]);
        assert_eq!(state.active, Some(1));
    }

    #[test]
    fn saving_under_a_name_again_replaces_and_nothing_saves_nothing() {
        let mut state = WindowState::default();
        assert!(!state.save_session("empty"), "no tabs, no session");
        state.open(tab("a"));
        assert!(state.save_session("s"));
        state.open(tab("b"));
        assert!(state.save_session("s"));
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].tabs.len(), 2);
        assert!(!state.save_session("   "));
        state.delete_session("s");
        assert!(state.sessions.is_empty());
    }

    #[test]
    fn replacing_drops_a_repeated_repository_and_keeps_the_wanted_front() {
        let mut state = WindowState::default();
        state.replace(vec![tab("a"), tab("b"), tab("a"), tab("c")], Some(3));
        assert_eq!(names(&state), ["a", "b", "c"]);
        assert_eq!(state.active, Some(2), "c, wherever it ends up");
    }

    #[test]
    fn the_state_survives_a_save_and_a_load() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut state = WindowState::default();
        state.open(tab("a"));
        state.save_session("one");
        save(dir.path(), &state).expect("saved");
        assert_eq!(load(dir.path()), state);
    }

    #[test]
    fn an_unreadable_file_is_moved_aside_and_the_window_starts_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join(FILE), "{ not json").expect("written");
        assert_eq!(load(dir.path()), WindowState::default());
        assert!(dir.path().join(BROKEN).is_file());
    }

    #[test]
    fn one_repository_reached_two_ways_is_one_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inner = dir.path().join("repo");
        std::fs::create_dir(&inner).expect("dir");
        let around = inner.join("..").join("repo");
        assert_eq!(normalize(&inner).unwrap(), normalize(&around).unwrap());
        let text = normalize(&inner).unwrap().to_string_lossy().into_owned();
        assert!(!text.starts_with(r"\\?\"), "{text}");
    }
}
