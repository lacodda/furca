//! The graph's lanes against what they must draw: every commit joined to
//! each of its parents by one unbroken lane, nothing joined that is not, a
//! branch's line kept in its column and colour, and a window of rows the same
//! whether it is drawn on its own or as part of the whole.
//!
//! The check plays the rows down from the top with nothing but the rows and
//! the commits' own parents, so it holds the layout to what a reader of the
//! picture sees rather than to how the layout happens to work.

mod fixture;

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use fixture::Layout;
use furca_core::{COLORS, Graph, Query, Repository, Row, Segment, SegmentKind, Tips};

/// A throwaway repository with a `git` that ignores the machine's own
/// configuration.
struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Self {
        let repo = Self {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_at(args, 0)
    }

    /// Runs `git` with its dates `minute` minutes into a fixed day, so the
    /// order by time is the test's to decide.
    fn git_at(&self, args: &[&str], minute: i64) -> String {
        let empty = self.path().join(".no-config");
        std::fs::write(&empty, "").expect("empty config");
        let date = format!("{} +0000", 1_700_000_000 + minute * 60);
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Author")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Carl Committer")
            .env("GIT_COMMITTER_EMAIL", "carl@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf-8")
            .trim()
            .to_owned()
    }

    /// An empty commit with these parents at `minute`,
    /// pointed at by `branch`; returns its id.
    fn commit(&self, branch: &str, parents: &[&str], subject: &str, minute: i64) -> String {
        let tree = self.git(&["hash-object", "-t", "tree", "-w", "--stdin"]);
        let mut args = vec!["commit-tree", tree.as_str(), "-m", subject];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        let id = self.git_at(&args, minute);
        self.git(&["update-ref", &format!("refs/heads/{branch}"), &id]);
        id
    }

    fn open(&self) -> Repository {
        Repository::open(self.path()).expect("furca-core opens the repository")
    }
}

/// The graph from `tips`, laid out to its end.
fn whole(repo: &Repository, tips: Tips) -> Graph {
    let mut graph = repo.graph(tips).expect("a graph begins");
    graph.extend(repo, usize::MAX).expect("the graph lays out");
    assert!(graph.is_complete());
    graph
}

/// Plays the rows of `graph` down from the top and checks every rule a
/// reader of the picture relies on.
fn check(repo: &Repository, graph: &Graph) {
    let rows = graph.rows(0..graph.len());
    let commits = graph
        .commits(repo, 0..graph.len())
        .expect("the commits read");
    assert_eq!(rows.len(), graph.len());
    assert_eq!(commits.len(), graph.len());
    let listed: HashSet<&str> = commits.iter().map(|commit| commit.id.as_str()).collect();
    let width = usize::from(graph.width());

    // What each column waits for below the row being checked, and its colour.
    let mut lanes: Vec<Option<(String, u8)>> = Vec::new();
    for (at, (row, commit)) in rows.iter().zip(&commits).enumerate() {
        let here = |rule: &str| format!("row {at} ({}): {rule}", &commit.id[..7]);
        assert!(
            row.color < COLORS,
            "{}",
            here("a colour is one of the theme's")
        );
        for line in &row.segments {
            assert!(
                usize::from(line.from.max(line.to)) < width,
                "{}",
                here("the graph is as wide as its widest row")
            );
        }

        // One line comes down for every open lane, in its own column.
        let down: Vec<&Segment> = row
            .segments
            .iter()
            .filter(|l| l.kind != SegmentKind::Out)
            .collect();
        let open: Vec<(usize, String, u8)> = lanes
            .iter()
            .enumerate()
            .filter_map(|(column, lane)| lane.clone().map(|(waits, color)| (column, waits, color)))
            .collect();
        assert_eq!(
            down.len(),
            open.len(),
            "{}",
            here("one line comes down per open lane")
        );
        let mut ending = Vec::new();
        for (line, (column, waits, color)) in down.iter().zip(&open) {
            assert_eq!(
                usize::from(line.from),
                *column,
                "{}",
                here("a lane comes down in its column")
            );
            assert_eq!(line.color, *color, "{}", here("a lane keeps its colour"));
            if *waits == commit.id {
                assert_eq!(
                    line.kind,
                    SegmentKind::In,
                    "{}",
                    here("a lane waiting for the commit ends at it")
                );
                assert_eq!(
                    line.to,
                    row.column,
                    "{}",
                    here("an ending lane ends at the commit")
                );
                ending.push(*column);
            } else {
                assert_eq!(
                    line.kind,
                    SegmentKind::Pass,
                    "{}",
                    here("other lanes pass it by")
                );
                assert_eq!(
                    line.to,
                    line.from,
                    "{}",
                    here("a passing lane keeps its column")
                );
            }
        }
        // The lines down to the parents come after the lines into the row.
        assert!(
            row.segments
                .iter()
                .skip_while(|l| l.kind != SegmentKind::Out)
                .all(|l| l.kind == SegmentKind::Out),
            "{}",
            here("the lines out come last")
        );

        let column = usize::from(row.column);
        if let Some(&first) = ending.first() {
            assert_eq!(
                column,
                first,
                "{}",
                here("a commit sits in the leftmost lane that waits for it")
            );
            assert_eq!(row.color, open.iter().find(|o| o.0 == first).unwrap().2);
        } else {
            let free = (0..)
                .find(|&c| lanes.get(c).is_none_or(Option::is_none))
                .unwrap();
            assert_eq!(
                column,
                free,
                "{}",
                here("a tip takes the leftmost free column")
            );
            if open.len() < usize::from(COLORS) {
                assert!(
                    open.iter().all(|o| o.2 != row.color),
                    "{}",
                    here("a new lane takes a colour no open lane has")
                );
            }
        }
        for column in ending {
            lanes[column] = None;
        }

        // One line goes down for every parent, in the order the commit names
        // them; a parent named twice is one line.
        let mut parents: Vec<&str> = Vec::new();
        for parent in &commit.parents {
            if listed.contains(parent.as_str()) && !parents.contains(&parent.as_str()) {
                parents.push(parent);
            }
        }
        let out: Vec<&Segment> = row
            .segments
            .iter()
            .filter(|l| l.kind == SegmentKind::Out)
            .collect();
        assert_eq!(
            out.len(),
            parents.len(),
            "{}",
            here("one line goes down per parent")
        );
        for (index, (line, parent)) in out.iter().zip(&parents).enumerate() {
            assert_eq!(
                line.from,
                row.column,
                "{}",
                here("lines go down from the commit")
            );
            let to = usize::from(line.to);
            let open_now = lanes.iter().filter(|lane| lane.is_some()).count();
            let waiting = lanes
                .iter()
                .position(|lane| lane.as_ref().is_some_and(|(waits, _)| waits == parent));
            if index == 0 {
                assert_eq!(
                    to,
                    column,
                    "{}",
                    here("the first parent continues the commit's lane")
                );
                assert_eq!(line.color, row.color, "{}", here("and keeps its colour"));
                assert!(lanes.get(to).is_none_or(Option::is_none));
            } else if let Some(waiting) = waiting {
                assert_eq!(
                    to,
                    waiting,
                    "{}",
                    here("a merge joins the lane already waiting for its parent")
                );
                assert_eq!(line.color, lanes[waiting].as_ref().unwrap().1);
            } else {
                let free = (column + 1..)
                    .find(|&c| lanes.get(c).is_none_or(Option::is_none))
                    .unwrap();
                assert_eq!(
                    to,
                    free,
                    "{}",
                    here("a merge opens a lane in the first free column right of it")
                );
                if open_now < usize::from(COLORS) {
                    assert!(
                        lanes
                            .iter()
                            .flatten()
                            .all(|(_, color)| *color != line.color),
                        "{}",
                        here("a new lane takes a colour no open lane has")
                    );
                }
            }
            if lanes.len() <= to {
                lanes.resize(to + 1, None);
            }
            lanes[to] = Some(((*parent).to_owned(), line.color));
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
    }

    assert!(
        lanes.iter().all(Option::is_none),
        "every lane ends at the commit it waits for"
    );
    for commit in &commits {
        for parent in &commit.parents {
            if repo.commit(parent).is_ok() {
                assert!(
                    listed.contains(parent.as_str()),
                    "a parent in the repository is in the graph"
                );
            }
        }
    }
}

fn line(kind: SegmentKind, from: u16, to: u16, color: u8) -> Segment {
    Segment {
        kind,
        from,
        to,
        color,
    }
}

#[test]
fn a_line_of_commits_is_one_lane() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    let b = repo.commit("main", &[&a], "b", 2);
    repo.commit("main", &[&b], "c", 3);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    let rows = graph.rows(0..3);
    let color = rows[0].color;
    use SegmentKind::*;
    assert_eq!(
        rows,
        vec![
            Row {
                column: 0,
                color,
                segments: vec![line(Out, 0, 0, color)]
            },
            Row {
                column: 0,
                color,
                segments: vec![line(In, 0, 0, color), line(Out, 0, 0, color)]
            },
            Row {
                column: 0,
                color,
                segments: vec![line(In, 0, 0, color)]
            },
        ]
    );
    assert_eq!(graph.width(), 1);
}

#[test]
fn a_merged_branch_runs_beside_the_mainline_down_to_where_it_forked() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    let b = repo.commit("main", &[&a], "b", 2);
    let f = repo.commit("feature", &[&a], "f", 3);
    repo.commit("main", &[&b, &f], "merge", 4);
    repo.git(&["branch", "-D", "feature"]);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    // merge, f, b, a - newest first.
    let rows = graph.rows(0..4);
    let (main, side) = (rows[0].color, rows[1].color);
    assert_ne!(main, side);
    use SegmentKind::*;
    assert_eq!(
        rows,
        vec![
            Row {
                column: 0,
                color: main,
                segments: vec![line(Out, 0, 0, main), line(Out, 0, 1, side)]
            },
            Row {
                column: 1,
                color: side,
                segments: vec![
                    line(Pass, 0, 0, main),
                    line(In, 1, 1, side),
                    line(Out, 1, 1, side)
                ]
            },
            Row {
                column: 0,
                color: main,
                segments: vec![
                    line(In, 0, 0, main),
                    line(Pass, 1, 1, side),
                    line(Out, 0, 0, main)
                ]
            },
            Row {
                column: 0,
                color: main,
                segments: vec![line(In, 0, 0, main), line(In, 1, 0, side)]
            },
        ]
    );
    assert_eq!(graph.width(), 2);
}

#[test]
fn a_merge_joins_the_lane_that_already_waits_for_its_parent() {
    // x has two children: c, which continues it, and the merge m, which
    // takes it as a second parent. c is newer, so its lane already waits for
    // x when m is drawn.
    let repo = Repo::new();
    let root = repo.commit("main", &[], "root", 1);
    let x = repo.commit("topic", &[&root], "x", 2);
    let y = repo.commit("main", &[&root], "y", 3);
    let m = repo.commit("main", &[&y, &x], "m", 4);
    repo.commit("topic", &[&x], "c", 5);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    let at_m = graph.position(&m).unwrap();
    let rows = graph.rows(0..graph.len());
    let out: Vec<&Segment> = rows[at_m]
        .segments
        .iter()
        .filter(|l| l.kind == SegmentKind::Out)
        .collect();
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].to, rows[0].column, "the merge leads into c's lane");
    assert_eq!(graph.width(), 2, "no third lane is opened for x");
}

#[test]
fn an_octopus_merge_opens_a_lane_per_parent() {
    let repo = Repo::new();
    let root = repo.commit("main", &[], "root", 1);
    let one = repo.commit("one", &[&root], "one", 2);
    let two = repo.commit("two", &[&root], "two", 3);
    let three = repo.commit("three", &[&root], "three", 4);
    repo.commit("main", &[&root, &one, &two, &three], "octopus", 5);
    for branch in ["one", "two", "three"] {
        repo.git(&["branch", "-D", branch]);
    }
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    let first = &graph.rows(0..1)[0];
    let targets: Vec<u16> = first.segments.iter().map(|l| l.to).collect();
    assert_eq!(targets, vec![0, 1, 2, 3]);
    let colors: HashSet<u8> = first.segments.iter().map(|l| l.color).collect();
    assert_eq!(colors.len(), 4, "four lanes, four colours");
}

#[test]
fn unrelated_histories_end_their_lanes_at_their_own_roots() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    repo.commit("main", &[&a], "b", 4);
    let other = repo.commit("pages", &[], "other root", 2);
    repo.commit("pages", &[&other], "other", 3);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);
    assert_eq!(graph.len(), 4);
}

#[test]
fn a_tip_takes_the_leftmost_free_column_not_the_far_end() {
    // m2 and b1 both wait for m1 in columns 0 and 1, c2 waits for c1 in
    // column 2. At m1 the lane in column 1 ends and leaves a hole between
    // two open lanes; the tip t below must take it.
    let repo = Repo::new();
    let root = repo.commit("main", &[], "root", 1);
    let m1 = repo.commit("main", &[&root], "m1", 7);
    repo.commit("main", &[&m1], "m2", 10);
    repo.commit("b", &[&m1], "b1", 9);
    let c1 = repo.commit("c", &[&root], "c1", 5);
    repo.commit("c", &[&c1], "c2", 8);
    let t = repo.commit("t", &[&root], "t", 6);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    let at = graph.position(&t).unwrap();
    assert_eq!(
        graph.rows(at..at + 1)[0].column,
        1,
        "the hole, not a fourth column"
    );
    assert_eq!(graph.width(), 3);
}

#[test]
fn a_merge_opens_its_new_lane_right_of_itself() {
    // The lane in column 0 ends at a root of another history, so column 0
    // is free when the merge below comes in column 1: its second parent
    // goes to column 2, to the right, not back across to column 0.
    let repo = Repo::new();
    let r1 = repo.commit("other", &[], "r1", 8);
    repo.commit("other", &[&r1], "r2", 10);
    let p2 = repo.commit("side", &[], "p2", 5);
    let p1 = repo.commit("main", &[], "p1", 6);
    let merge = repo.commit("main", &[&p1, &p2], "merge", 7);
    repo.commit("y", &[&merge], "y", 9);
    repo.git(&["branch", "-D", "side"]);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);

    let at = graph.position(&merge).unwrap();
    let row = &graph.rows(at..at + 1)[0];
    assert_eq!(row.column, 1);
    let out: Vec<u16> = row
        .segments
        .iter()
        .filter(|s| s.kind == SegmentKind::Out)
        .map(|s| s.to)
        .collect();
    assert_eq!(out, vec![1, 2]);
}

#[test]
fn a_lane_no_commit_sits_in_still_counts_in_the_width() {
    // The merge's second parent is its first parent's own parent: the lane
    // opened for it runs beside the mainline and ends at it in column 0, so
    // no commit ever sits in column 1 - and the graph is still two wide.
    let repo = Repo::new();
    let c = repo.commit("main", &[], "c", 1);
    let b = repo.commit("main", &[&c], "b", 2);
    repo.commit("main", &[&b, &c], "a", 3);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);
    assert!(graph.rows(0..graph.len()).iter().all(|row| row.column == 0));
    assert_eq!(graph.width(), 2);
}

#[test]
fn a_shallow_clone_ends_its_lanes_where_the_history_was_cut() {
    let origin = Repo::new();
    let mut parent = origin.commit("main", &[], "a", 1);
    for (index, subject) in ["b", "c", "d"].iter().enumerate() {
        parent = origin.commit("main", &[&parent], subject, 2 + index as i64);
    }
    origin.git(&["checkout", "--quiet", "--force", "main"]);
    let clone = tempfile::tempdir().expect("temp dir");
    let source = origin.path().display().to_string().replace('\\', "/");
    // `file:///C:/...` on Windows, `file:///tmp/...` elsewhere.
    let url = format!("file:///{}", source.trim_start_matches('/'));
    origin.git(&[
        "clone",
        "--quiet",
        "--depth",
        "2",
        &url,
        clone.path().to_str().unwrap(),
    ]);
    let opened = Repository::open(clone.path()).expect("open the clone");
    let graph = whole(&opened, Tips::All);
    assert_eq!(graph.len(), 2);
    check(&opened, &graph);
    let last = &graph.rows(1..2)[0];
    assert!(last.segments.iter().all(|l| l.kind != SegmentKind::Out));
}

#[test]
fn a_parent_named_twice_is_one_line() {
    // git will not write such a commit itself, but an old import can hold
    // one: written here byte for byte, past git's checks.
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    let tree = repo.git(&["rev-parse", &format!("{a}^{{tree}}")]);
    let body = [
        format!("tree {tree}"),
        format!("parent {a}"),
        format!("parent {a}"),
        "author Ada <ada@example.com> 1700000120 +0000".to_owned(),
        "committer Ada <ada@example.com> 1700000120 +0000".to_owned(),
        String::new(),
        "twice".to_owned(),
        String::new(),
    ]
    .join("\n");
    let path = repo.path().join(".commit");
    std::fs::write(&path, body).expect("write the commit");
    let twice = repo.git(&[
        "hash-object",
        "-t",
        "commit",
        "-w",
        "--literally",
        path.to_str().unwrap(),
    ]);
    repo.git(&["update-ref", "refs/heads/main", &twice]);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);
    let first = &graph.rows(0..1)[0];
    assert_eq!(
        first.segments.len(),
        1,
        "one line down, not two on top of each other"
    );
}

#[test]
fn a_tag_on_a_tree_is_not_a_place_the_graph_starts() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    let tree = repo.git(&["rev-parse", &format!("{a}^{{tree}}")]);
    repo.git(&["tag", "tree-tag", &tree]);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    check(&opened, &graph);
    assert_eq!(graph.len(), 1);
}

#[test]
fn a_graph_of_one_branch_holds_only_its_history() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    let b = repo.commit("main", &[&a], "b", 2);
    let f = repo.commit("feature", &[&a], "f", 3);
    let opened = repo.open();

    let graph = whole(&opened, Tips::Revisions(vec!["refs/heads/feature".into()]));
    check(&opened, &graph);
    assert_eq!(graph.id(0).as_deref(), Some(f.as_str()));
    assert_eq!(graph.id(1).as_deref(), Some(a.as_str()));
    assert_eq!(graph.len(), 2);
    assert_eq!(graph.position(&b), None);

    let error = opened.graph(Tips::Revisions(vec!["no-such-branch".into()]));
    assert!(
        error.is_err(),
        "a name that names nothing is an error, not an empty graph"
    );
}

#[test]
fn a_graph_knows_when_the_refs_moved_under_it() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "a", 1);
    repo.commit("feature", &[&a], "f", 2);
    let opened = repo.open();
    let all = whole(&opened, Tips::All);
    let feature = whole(&opened, Tips::Revisions(vec!["refs/heads/feature".into()]));
    assert!(opened.graph_is_current(&all).unwrap());
    assert!(opened.graph_is_current(&feature).unwrap());

    repo.commit("main", &[&a], "b", 3);
    let opened = repo.open();
    assert!(
        !opened.graph_is_current(&all).unwrap(),
        "a new commit on main"
    );
    assert!(
        opened.graph_is_current(&feature).unwrap(),
        "feature did not move"
    );

    repo.git(&["branch", "-D", "feature"]);
    let opened = repo.open();
    assert!(
        !opened.graph_is_current(&feature).unwrap(),
        "the branch is gone"
    );
}

#[test]
fn a_search_finds_words_in_the_subject_and_the_author_and_the_start_of_an_id() {
    let repo = Repo::new();
    let a = repo.commit("main", &[], "Fix the PATH handling", 1);
    let b = repo.commit("main", &[&a], "add a ledger", 2);
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    let find = |text: &str| {
        graph
            .find(&opened, &Query::new(text).expect("a query"), 0..graph.len())
            .expect("the search runs")
    };
    let (at_a, at_b) = (graph.position(&a).unwrap(), graph.position(&b).unwrap());

    assert_eq!(find("path"), vec![at_a], "without case");
    assert_eq!(find("LEDGER"), vec![at_b]);
    assert_eq!(
        find("ada"),
        vec![at_b.min(at_a), at_b.max(at_a)],
        "the author's name"
    );
    assert_eq!(find("example.com").len(), 2, "the author's address");
    assert_eq!(find(&b[..7]), vec![at_b], "the start of an id");
    assert_eq!(
        find(&b[..7].to_uppercase()),
        vec![at_b],
        "an id in capitals"
    );
    assert!(find("nothing like it").is_empty());
    assert_eq!(Query::new("   "), None, "a blank query looks for nothing");

    // Rows by commit: an id the graph does not hold is left out.
    assert_eq!(
        graph.positions(&[b.clone(), "not an id".into(), a.clone()]),
        vec![at_b.min(at_a), at_b.max(at_a)]
    );
    assert!(graph.positions(&["0".repeat(40)]).is_empty());

    // A window of the rows is searched alone, and answers in rows of the
    // whole graph.
    let older = at_a.max(at_b);
    let only = graph
        .find(&opened, &Query::new("ada").unwrap(), older..older + 1)
        .unwrap();
    assert_eq!(only, vec![older]);
}

#[test]
fn an_empty_repository_has_an_empty_graph() {
    let repo = Repo::new();
    let opened = repo.open();
    let graph = whole(&opened, Tips::All);
    assert!(graph.is_empty());
    assert!(graph.rows(0..10).is_empty());
    assert_eq!(graph.width(), 0);
}

fn fixture_graph(layout: Layout) {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = fixture::ensure(&root, 3_000, layout);
    let repo = Repository::open(&path).expect("open the fixture");
    let graph = whole(&repo, Tips::All);
    check(&repo, &graph);

    // Laid out a few rows at a time, the graph is the same graph.
    let mut stepwise = repo.graph(Tips::All).expect("a graph begins");
    while stepwise.extend(&repo, 7).expect("the graph lays out") > 0 {}
    assert!(stepwise.is_complete());
    assert_eq!(stepwise.len(), graph.len());
    let all = graph.rows(0..graph.len());
    assert_eq!(stepwise.rows(0..stepwise.len()), all);

    // A window is drawn the same on its own as within the whole: every row
    // alone, each played forward from the mark before it, and windows across
    // marks and past the end.
    for at in 0..graph.len() {
        assert_eq!(graph.rows(at..at + 1), all[at..at + 1], "row {at}");
    }
    for range in [63..65, 64..128, 100..300, graph.len() - 5..graph.len() + 10] {
        let end = range.end.min(graph.len());
        assert_eq!(
            graph.rows(range.clone()),
            all[range.start..end],
            "rows {range:?}"
        );
    }
}

#[test]
fn the_fixtures_graph_draws_its_history_with_a_commit_graph() {
    fixture_graph(Layout::PackedWithGraph);
}

#[test]
fn the_fixtures_graph_draws_its_history_without_a_commit_graph() {
    fixture_graph(Layout::Packed);
}

#[test]
fn a_graph_can_be_handed_to_another_thread() {
    fn send<T: Send>() {}
    send::<Graph>();
}
