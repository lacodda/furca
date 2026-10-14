//! Installing a release with the repository's own installer, and proving
//! what got installed.
//!
//! The installers are the line's pair: `tools/install.ps1` on Windows and
//! `tools/install.sh` elsewhere, pinned to a tag by `{APP}_VERSION`. What
//! they did is not taken on their word. The binary the PATH finds - or the
//! one in the directory installed into - has to print the version, hash the
//! same as the binary inside the release page's archive for this machine's
//! target, and so does every alias next to it. A version alone proves
//! little: a local build of the same number prints the same.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use semver::Version;
use sha2::{Digest, Sha256};

use crate::hosting::Forge;
use crate::plan::{Check, Status};

/// The target this furca was built for: the archive the proof downloads is
/// the one built for the same machine.
pub const TARGET: &str = env!("FURCA_RELEASE_TARGET");

/// The installers, relative to the root: the line's standard pair.
pub const INSTALLERS: [&str; 2] = ["tools/install.ps1", "tools/install.sh"];

/// What the steps after the tag ask of the machine they run on.
pub trait Machine {
    /// Runs `program` with `args` from `dir`, `env` added to its
    /// environment, its output on the terminal. The exit code; `None` when
    /// it was killed. `ErrorKind::NotFound` when there is no such program.
    fn run(
        &self,
        program: &str,
        args: &[&str],
        dir: &Path,
        env: &[(String, String)],
    ) -> std::io::Result<Option<i32>>;

    /// The file the command `name` runs, as the PATH finds it.
    fn find(&self, name: &str) -> Option<PathBuf>;

    /// What `program --version` prints.
    fn version(&self, program: &Path) -> Result<String, String>;
}

/// The machine this process runs on.
pub struct Local {
    /// Sends what the programs print on their standard output to standard
    /// error instead, for a caller whose standard output is one document.
    pub to_stderr: bool,
}

impl Machine for Local {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        dir: &Path,
        env: &[(String, String)],
    ) -> std::io::Result<Option<i32>> {
        let mut command = Command::new(program);
        command.args(args).current_dir(dir);
        for (key, value) in env {
            command.env(key, value);
        }
        if self.to_stderr {
            command.stdout(Stdio::from(std::io::stderr()));
        }
        Ok(command.status()?.code())
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        let names = executable_names(name);
        std::env::split_paths(&path)
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
            .find(|candidate| candidate.is_file())
    }

    fn version(&self, program: &Path) -> Result<String, String> {
        let output = Command::new(program)
            .arg("--version")
            .output()
            .map_err(|e| format!("{} did not start: {e}", program.display()))?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() {
            Ok(text)
        } else {
            Err(format!(
                "{} --version exited {}",
                program.display(),
                output
                    .status
                    .code()
                    .map_or("by a signal".to_owned(), |c| c.to_string())
            ))
        }
    }
}

/// The file names a command `name` may have here: `name.exe` and the other
/// PATHEXT extensions on Windows, `name` elsewhere.
fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_owned());
        extensions
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| format!("{name}{}", e.to_ascii_lowercase()))
            .collect()
    } else {
        vec![name.to_owned()]
    }
}

/// The name of a binary file here: `name.exe` on Windows.
pub(crate) fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The prefix of an installer's environment: `FURCA` for `furca`, so
/// `FURCA_VERSION`, `FURCA_INSTALL_DIR`, `FURCA_NO_PATH`.
pub(crate) fn env_prefix(bin: &str) -> String {
    bin.to_ascii_uppercase().replace('-', "_")
}

/// Where the installed program is looked for.
#[derive(Debug, Clone, Copy)]
pub enum InstallTo<'a> {
    /// The owner's own place, found through the PATH.
    Owner,
    /// A directory of the caller's, which the installer is told to install
    /// into and to leave off the PATH.
    Dir(&'a Path),
}

/// One install to make and prove.
pub(crate) struct Order<'a> {
    pub root: &'a Path,
    /// The program the installer puts in place.
    pub bin: &'a str,
    /// The other names it answers to, installed next to it.
    pub aliases: &'a [String],
    pub tag: &'a str,
    pub version: &'a Version,
    pub into: InstallTo<'a>,
}

/// Runs the repository's installer for the order's tag and proves what it
/// installed. Every check is returned; the first that fails ends the list.
pub(crate) fn install(
    order: &Order,
    forge: &dyn Forge,
    machine: &dyn Machine,
    say: &mut dyn FnMut(&str),
) -> Vec<Check> {
    let Order {
        root,
        bin,
        aliases,
        tag,
        version,
        into,
    } = *order;
    let mut checks = Vec::new();
    let (program, args, installer): (&str, Vec<String>, &str) = if cfg!(windows) {
        let script = root.join(INSTALLERS[0]);
        (
            "powershell",
            vec![
                "-NoProfile".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
                script.display().to_string(),
            ],
            INSTALLERS[0],
        )
    } else {
        let script = root.join(INSTALLERS[1]);
        ("sh", vec![script.display().to_string()], INSTALLERS[1])
    };
    let prefix = env_prefix(bin);
    let mut env = vec![(format!("{prefix}_VERSION"), tag.to_owned())];
    if let InstallTo::Dir(dir) = into {
        env.push((format!("{prefix}_INSTALL_DIR"), dir.display().to_string()));
        env.push((format!("{prefix}_NO_PATH"), "1".to_owned()));
    }
    say(&format!("{installer} for {tag}"));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let ran = machine.run(program, &args, root, &env);
    let exit = match ran {
        Ok(Some(0)) => None,
        Ok(Some(code)) => Some(format!("exited {code}")),
        Ok(None) => Some("was stopped".to_owned()),
        Err(error) => Some(format!("did not start: {error}")),
    };
    if let Some(exit) = exit {
        checks.push(fail("installer", format!("{installer} {exit}"), Vec::new()));
        return checks;
    }
    checks.push(pass("installer", format!("{installer} installed {tag}")));

    // Where the program and its aliases are now.
    let locate = |name: &str| -> Option<PathBuf> {
        match into {
            InstallTo::Owner => machine.find(name),
            InstallTo::Dir(dir) => Some(dir.join(binary_name(name))).filter(|p| p.is_file()),
        }
    };
    let Some(path) = locate(bin) else {
        let summary = match into {
            InstallTo::Owner => format!(
                "`{bin}` is not on the PATH: open a new terminal if the installer just added its directory, then resume"
            ),
            InstallTo::Dir(dir) => format!(
                "the installer put no {} in {}",
                binary_name(bin),
                dir.display()
            ),
        };
        checks.push(fail("found", summary, Vec::new()));
        return checks;
    };
    checks.push(pass("found", path.display().to_string()));

    let said = machine.version(&path);
    match &said {
        Ok(text) if says_version(text, version) => {
            checks.push(pass("version", format!("{} says `{text}`", path.display())));
        }
        Ok(text) => {
            checks.push(fail(
                "version",
                format!("{} says `{text}`, not {version}", path.display()),
                vec!["another copy earlier on the PATH, or an installer that ignores its pinned version".to_owned()],
            ));
            return checks;
        }
        Err(why) => {
            checks.push(fail("version", why.clone(), Vec::new()));
            return checks;
        }
    }

    let installed = match sha256_of(&path) {
        Ok(hash) => hash,
        Err(why) => {
            checks.push(fail("build", why, Vec::new()));
            return checks;
        }
    };
    say(&format!(
        "comparing {} with the release's own build",
        path.display()
    ));
    match released_build(forge, tag, bin) {
        Ok(Some((asset, hash))) if hash == installed => checks.push(Check {
            name: "build",
            status: Status::Pass,
            summary: format!("the release's own build, as in {asset}"),
            details: vec![format!("sha256 {installed}")],
        }),
        Ok(Some((asset, hash))) => {
            checks.push(fail(
                "build",
                format!("{} is not the build in {asset}", path.display()),
                vec![
                    format!("installed sha256 {installed}"),
                    format!("released  sha256 {hash}"),
                ],
            ));
            return checks;
        }
        Ok(None) => {
            checks.push(fail(
                "build",
                format!("the release page of {tag} has no archive for {TARGET} with {bin} in it"),
                Vec::new(),
            ));
            return checks;
        }
        Err(why) => {
            checks.push(fail("build", why, Vec::new()));
            return checks;
        }
    }

    if aliases.is_empty() {
        return checks;
    }
    let mut wrong = Vec::new();
    for alias in aliases {
        match locate(alias) {
            None => wrong.push(format!("`{alias}` is not there")),
            Some(found) => match sha256_of(&found) {
                Ok(hash) if hash == installed => {}
                Ok(_) => wrong.push(format!(
                    "{} is another build than {}",
                    found.display(),
                    path.display()
                )),
                Err(why) => wrong.push(why),
            },
        }
    }
    checks.push(if wrong.is_empty() {
        pass(
            "aliases",
            format!("{} answer with the same build", aliases.join(", ")),
        )
    } else {
        fail("aliases", "an alias lags behind".to_owned(), wrong)
    });
    checks
}

/// Whether `--version` output names `version`: as a word of its own,
/// `1.2.3` or `v1.2.3`.
fn says_version(text: &str, version: &Version) -> bool {
    let want = version.to_string();
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .any(|word| word.strip_prefix('v').unwrap_or(word) == want)
}

/// The SHA-256 of the binary `bin` inside the release page's archive for
/// this machine's target, and the archive's name; `None` when no archive
/// for the target holds it.
fn released_build(
    forge: &dyn Forge,
    tag: &str,
    bin: &str,
) -> Result<Option<(String, String)>, String> {
    let page = forge
        .page(tag)?
        .ok_or_else(|| format!("{tag} has no release page"))?;
    let archives = page
        .assets
        .iter()
        .filter(|a| a.contains(TARGET) && archive_kind(a).is_some());
    for asset in archives {
        let bytes = forge.download(tag, asset)?;
        let dir = tempfile::tempdir().map_err(|e| format!("no temp directory: {e}"))?;
        let file = dir.path().join(asset);
        std::fs::write(&file, bytes).map_err(|e| format!("could not keep {asset}: {e}"))?;
        let out = dir.path().join("unpacked");
        std::fs::create_dir(&out).map_err(|e| format!("no temp directory: {e}"))?;
        unpack(&file, &out)?;
        if let Some(found) = find_file(&out, &binary_name(bin)) {
            return Ok(Some((asset.clone(), sha256_of(&found)?)));
        }
    }
    Ok(None)
}

/// An archive the proof can open: a zip or a gzipped tar.
fn archive_kind(name: &str) -> Option<&'static str> {
    if name.ends_with(".zip") {
        Some("zip")
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some("tar.gz")
    } else {
        None
    }
}

/// Unpacks `archive` into `into` with the system's own tools, as the
/// installers do: Windows' `tar` (bsdtar, which reads zip as well), `tar`
/// elsewhere and `unzip` for a zip there.
pub(crate) fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let zip = archive_kind(&archive.to_string_lossy()) == Some("zip");
    let mut command = if cfg!(windows) {
        // Git Bash puts its GNU tar first on the PATH, and it reads `C:` as
        // a remote host: Windows' own is called by its full path.
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut command = Command::new(Path::new(&root).join("System32").join("tar.exe"));
        command.arg("-xf").arg(archive).arg("-C").arg(into);
        command
    } else if zip {
        let mut command = Command::new("unzip");
        command.arg("-q").arg(archive).arg("-d").arg(into);
        command
    } else {
        let mut command = Command::new("tar");
        command.arg("-xzf").arg(archive).arg("-C").arg(into);
        command
    };
    let output = command
        .output()
        .map_err(|e| format!("could not unpack {}: {e}", archive.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not unpack {}: {}",
            archive.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// The first file named `name` under `dir`, depth first.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in &entries {
        let path = entry.path();
        if path.is_file() && entry.file_name().to_string_lossy() == name {
            return Some(path);
        }
    }
    entries
        .iter()
        .filter(|e| e.path().is_dir())
        .find_map(|e| find_file(&e.path(), name))
}

/// The SHA-256 of a file, in lower-case hex.
pub(crate) fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// The target triples a text names - `x86_64-pc-windows-msvc`,
/// `aarch64-apple-darwin` - and the tails of ones it builds from a variable,
/// such as `$ARCH-unknown-linux-musl`, which come back as
/// `-unknown-linux-musl`.
pub(crate) fn targets(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let part = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
    let mut found: Vec<String> = Vec::new();
    for vendor in ["-pc-", "-unknown-", "-apple-"] {
        let mut from = 0;
        while let Some(at) = text[from..].find(vendor) {
            let start = at + from;
            from = start + vendor.len();
            let mut left = start;
            while left > 0 && part(bytes[left - 1]) {
                left -= 1;
            }
            // The system, then an optional environment: two parts at most.
            let mut right = start + vendor.len();
            let mut parts = 0;
            while parts < 2 {
                let begin = right;
                while right < bytes.len() && part(bytes[right]) {
                    right += 1;
                }
                if right == begin {
                    break;
                }
                parts += 1;
                if right < bytes.len() && bytes[right] == b'-' && parts < 2 {
                    let next = right + 1;
                    if next < bytes.len() && part(bytes[next]) {
                        right = next;
                        continue;
                    }
                }
                break;
            }
            if parts == 0 {
                continue;
            }
            let triple = text[left..right].trim_end_matches('-').to_owned();
            if !found.contains(&triple) {
                found.push(triple);
            }
        }
    }
    found
}

fn pass(name: &'static str, summary: String) -> Check {
    Check {
        name,
        status: Status::Pass,
        summary,
        details: Vec::new(),
    }
}

fn fail(name: &'static str, summary: String, details: Vec<String>) -> Check {
    Check {
        name,
        status: Status::Fail,
        summary,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installer_names_its_targets_whole_or_by_their_tail() {
        let ps1 = r#"$name = "furca-$tag-x86_64-pc-windows-msvc"
$url = "https://example.com/$name.zip""#;
        assert_eq!(targets(ps1), ["x86_64-pc-windows-msvc"]);
        let sh = r#"case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
    Darwin-arm64) TARGET="aarch64-apple-darwin" ;;
esac
URL="$BASE/$NAME.tar.gz"
LIBC="$ARCH-unknown-linux-musl""#;
        assert_eq!(
            targets(sh),
            [
                "x86_64-unknown-linux-gnu",
                "-unknown-linux-musl",
                "aarch64-apple-darwin"
            ]
        );
        assert!(targets("the pc-unknown words of prose").is_empty());
    }

    #[test]
    fn a_version_is_read_as_a_word_of_its_own() {
        let v = Version::new(0, 5, 0);
        assert!(says_version("furca 0.5.0", &v));
        assert!(says_version("turnout v0.5.0 (abc1234)", &v));
        assert!(!says_version("furca 0.5.01", &v));
        assert!(!says_version("furca 10.5.0", &v));
    }

    #[test]
    fn a_file_is_hashed_and_found_in_a_tree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("demo-v1.0.0").join("bin");
        std::fs::create_dir_all(&nested).expect("dirs");
        std::fs::write(nested.join("demo"), b"abc").expect("write");
        let found = find_file(dir.path(), "demo").expect("found");
        assert_eq!(
            sha256_of(&found).expect("hashes"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(find_file(dir.path(), "other"), None);
        assert_eq!(env_prefix("kasl-server"), "KASL_SERVER");
    }
}
