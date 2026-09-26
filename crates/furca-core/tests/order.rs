//! The graph order against git's own: `log(Tips::All, ..)` must list exactly
//! what `git log --all --date-order` lists, in the same order, on a history
//! with merges, open branches, tags and commits from slow clocks.
//!
//! Run on both layouts, because they take different paths through the walk:
//! with a commit-graph it explores lazily by generation, without one every
//! commit counts as infinitely high and the walk explores everything first.

mod fixture;

use fixture::Layout;
use furca_core::{Repository, Tips};

const COMMITS: u32 = 3_000;

fn git_order(path: &std::path::Path) -> Vec<String> {
    let output = fixture::git_command(path)
        .args(["log", "--all", "--date-order", "--format=%H"])
        .output()
        .expect("git log runs");
    assert!(output.status.success(), "git log failed");
    String::from_utf8(output.stdout)
        .expect("utf-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn check(layout: Layout) {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let path = fixture::ensure(&root, COMMITS, layout);

    let expected = git_order(&path);
    assert!(
        expected.len() >= COMMITS as usize,
        "the fixture holds {} commits, fewer than asked",
        expected.len()
    );

    let log = Repository::open(&path)
        .expect("open")
        .log(Tips::All, usize::MAX)
        .expect("walk");
    let ours: Vec<String> = log.commits.into_iter().map(|c| c.id).collect();
    assert!(!log.truncated);

    if let Some(at) = ours.iter().zip(&expected).position(|(a, b)| a != b) {
        panic!(
            "on {}: first difference at position {at}: furca lists {}, git lists {}",
            layout.name(),
            ours[at],
            expected[at]
        );
    }
    assert_eq!(ours.len(), expected.len(), "on {}", layout.name());
}

#[test]
fn the_order_is_gits_date_order_with_a_commit_graph() {
    check(Layout::PackedWithGraph);
}

#[test]
fn the_order_is_gits_date_order_without_a_commit_graph() {
    check(Layout::Packed);
}
