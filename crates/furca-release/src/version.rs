//! Version numbers: what the commits ask for and which numbers are free.

use semver::Version;
use serde::Serialize;

/// How far a release moves the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

impl Bump {
    /// The version after `from`. Before 1.0 a breaking change moves the
    /// minor number: 0.x is where the API is still allowed to change, and
    /// Cargo already reads 0.2 -> 0.3 as incompatible.
    pub fn apply(self, from: &Version) -> Version {
        match self {
            Bump::Major if from.major > 0 => Version::new(from.major + 1, 0, 0),
            Bump::Major | Bump::Minor => Version::new(from.major, from.minor + 1, 0),
            Bump::Patch => Version::new(from.major, from.minor, from.patch + 1),
        }
    }

    /// The bump that turns `from` into `to`, if `to` is one of the numbers
    /// right after `from`: anything else skips a number or goes backwards.
    pub fn between(from: &Version, to: &Version) -> Option<Bump> {
        let to = Version::new(to.major, to.minor, to.patch);
        if to == Version::new(from.major, from.minor, from.patch + 1) {
            Some(Bump::Patch)
        } else if to == Version::new(from.major, from.minor + 1, 0) {
            Some(Bump::Minor)
        } else if to == Version::new(from.major + 1, 0, 0) {
            Some(Bump::Major)
        } else {
            None
        }
    }

    /// What a release of `to` after `from` means, judged by the numbers:
    /// before 1.0 a minor step already allows a breaking change.
    pub fn covers(self, needed: Bump, from: &Version) -> bool {
        if from.major == 0 && needed == Bump::Major {
            return self >= Bump::Minor;
        }
        self >= needed
    }
}

/// The tag a version is released under. The line tags `v` and the number,
/// and nothing else.
pub fn tag(version: &Version) -> String {
    format!("v{version}")
}

/// The version a tag names, if it is a release tag at all.
pub fn from_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v')?).ok()
}

/// A version with its build metadata dropped, for comparing what a registry
/// counts as one number.
pub fn plain(version: &Version) -> Version {
    let mut plain = version.clone();
    plain.build = semver::BuildMetadata::EMPTY;
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn before_one_a_breaking_change_moves_the_minor_number() {
        assert_eq!(Bump::Major.apply(&v("0.2.3")), v("0.3.0"));
        assert_eq!(Bump::Major.apply(&v("1.2.3")), v("2.0.0"));
        assert_eq!(Bump::Minor.apply(&v("1.2.3")), v("1.3.0"));
        assert_eq!(Bump::Patch.apply(&v("1.2.3")), v("1.2.4"));
    }

    #[test]
    fn only_the_next_numbers_are_steps() {
        let from = v("0.5.0");
        assert_eq!(Bump::between(&from, &v("0.5.1")), Some(Bump::Patch));
        assert_eq!(Bump::between(&from, &v("0.6.0")), Some(Bump::Minor));
        assert_eq!(Bump::between(&from, &v("1.0.0")), Some(Bump::Major));
        assert_eq!(Bump::between(&from, &v("0.7.0")), None, "a hole");
        assert_eq!(Bump::between(&from, &v("0.5.0")), None, "taken");
        assert_eq!(Bump::between(&from, &v("0.4.0")), None, "backwards");
    }

    #[test]
    fn a_minor_step_covers_a_breaking_change_only_before_one() {
        assert!(Bump::Minor.covers(Bump::Major, &v("0.2.0")));
        assert!(!Bump::Minor.covers(Bump::Major, &v("1.2.0")));
        assert!(!Bump::Patch.covers(Bump::Minor, &v("0.2.0")));
        assert!(Bump::Major.covers(Bump::Patch, &v("1.0.0")));
    }

    #[test]
    fn only_v_and_a_version_is_a_release_tag() {
        assert_eq!(from_tag("v0.2.0"), Some(v("0.2.0")));
        assert_eq!(from_tag("v1.0.0-rc.1"), Some(v("1.0.0-rc.1")));
        assert_eq!(from_tag("0.2.0"), None);
        assert_eq!(from_tag("release-1"), None);
        assert_eq!(from_tag("v1.2"), None);
    }
}
