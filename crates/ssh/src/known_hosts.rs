//! OpenSSH `known_hosts` parsing, host matching, and appending.
//!
//! russh's own matcher ignores `@revoked`, wildcards, and negation, and
//! aborts on one malformed line, so dbmcp matches entries itself on top of
//! [`ssh_key::known_hosts::Entry`].

use std::fs::OpenOptions;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::Path;

use ring::hmac;
use russh::keys::ssh_key::known_hosts::{Entry, HostPatterns, Marker};
use russh::keys::{Algorithm, PublicKey};
use tracing::debug;

/// Parsed `known_hosts` file.
///
/// Malformed lines are skipped; every kept entry remembers its line number.
#[derive(Debug, Default)]
pub(crate) struct KnownHosts {
    entries: Vec<Line>,
}

/// One parsed entry and the 1-based line it came from.
#[derive(Debug)]
struct Line {
    number: usize,
    entry: Entry,
}

/// Result of looking up a presented host key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    /// The key is marked `@revoked` for this host on `line`.
    Revoked { line: usize },
    /// The key is recorded for this host.
    Match,
    /// The host is recorded, but only with other keys; `line` is the first.
    ///
    /// Covers keys of another type too, so `accept-new` cannot be steered
    /// into learning an attacker's key by offering a different algorithm.
    Changed { line: usize },
    /// No key is recorded for the host.
    Unknown,
}

/// Returns the name `known_hosts` records for `host:port`.
///
/// Port 22 is the bare lowercase host; any other port is `[host]:port`.
pub(crate) fn host_name(host: &str, port: u16) -> String {
    let host = host.to_ascii_lowercase();
    if port == 22 { host } else { format!("[{host}]:{port}") }
}

impl KnownHosts {
    /// Reads and parses `path`; a missing file has no entries.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the file exists but cannot be read.
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Self::parse(&String::from_utf8_lossy(&bytes))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Parses `known_hosts` text, skipping blank, comment, and malformed lines.
    pub(crate) fn parse(text: &str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut entries = Vec::new();
        for (index, raw) in text.split('\n').enumerate() {
            let number = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Fields may be separated by any run of spaces or tabs.
            let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
            match line.parse::<Entry>() {
                Ok(entry) => entries.push(Line { number, entry }),
                Err(e) => debug!("skipping malformed known_hosts line {number}: {e}"),
            }
        }
        Self { entries }
    }

    /// Looks up `key` presented by the host recorded as `name`.
    ///
    /// `name` comes from [`host_name`], so it is lowercase. Revocation wins over a match, and a
    /// match against any recorded key wins over a change.
    pub(crate) fn check(&self, name: &str, key: &PublicKey) -> Check {
        let mut changed = None;
        let mut matched = false;
        for line in self.lines_for(name) {
            let same_key = line.entry.public_key().key_data() == key.key_data();
            match line.entry.marker() {
                Some(Marker::Revoked) if same_key => return Check::Revoked { line: line.number },
                None if same_key => matched = true,
                None => {
                    changed.get_or_insert(line.number);
                }
                // `@cert-authority` is unsupported; other revoked keys are irrelevant.
                Some(_) => {}
            }
        }
        match (matched, changed) {
            (true, _) => Check::Match,
            (false, Some(line)) => Check::Changed { line },
            (false, None) => Check::Unknown,
        }
    }

    /// Keeps only the entries whose host patterns match `name`.
    pub(crate) fn for_host(mut self, name: &str) -> Self {
        self.entries
            .retain(|line| hosts_match(line.entry.host_patterns(), name));
        self
    }

    /// Returns the key algorithms trusted for `name`.
    pub(crate) fn algorithms(&self, name: &str) -> Vec<Algorithm> {
        self.lines_for(name)
            .filter(|line| line.entry.marker().is_none())
            .map(|line| line.entry.public_key().algorithm())
            .collect()
    }

    /// Iterates entries whose host patterns match `name`.
    fn lines_for<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Line> {
        self.entries
            .iter()
            .filter(move |line| hosts_match(line.entry.host_patterns(), name))
    }
}

/// Reorders `default` so algorithms recorded for the host come first.
///
/// Without this, russh negotiates Ed25519 and a host recorded only with
/// ECDSA or RSA would look unknown. Recorded RSA keys promote every
/// `rsa-sha2-*` variant.
pub(crate) fn preferred(recorded: &[Algorithm], default: &[Algorithm]) -> Vec<Algorithm> {
    let is_rsa = |a: &Algorithm| matches!(a, Algorithm::Rsa { .. });
    let is_recorded = |a: &Algorithm| recorded.iter().any(|r| r == a || (is_rsa(r) && is_rsa(a)));
    let (mut first, rest): (Vec<_>, Vec<_>) = default.iter().cloned().partition(is_recorded);
    first.extend(rest);
    first
}

/// Appends an unhashed entry for `name` and `key` to `path`.
///
/// Creates the file and its parent directories when missing (0600 and
/// 0700 on unix; elsewhere they inherit the parent's permissions). A
/// newline is written first when the file does not end with one.
///
/// # Errors
///
/// Returns the I/O error when the directory or file cannot be created,
/// read, or written.
pub(crate) fn append(path: &Path, name: &str, key: &PublicKey) -> io::Result<()> {
    let entry = entry(name, key).map_err(io::Error::other)?;

    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(dir)?;
    }

    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;

    let mut line = String::new();
    if file.metadata()?.len() > 0 {
        let mut last = [0; 1];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            line.push('\n');
        }
    }
    line.push_str(&entry);
    line.push('\n');
    file.write_all(line.as_bytes())
}

/// Renders the unhashed, comment-free `known_hosts` entry for `name` and `key`.
///
/// # Errors
///
/// Returns the encoding error when `key` cannot be serialised.
pub(crate) fn entry(name: &str, key: &PublicKey) -> russh::keys::ssh_key::Result<String> {
    let key = PublicKey::new(key.key_data().clone(), "").to_openssh()?;
    Ok(format!("{name} {key}"))
}

/// Returns whether `patterns` match `name`, honouring `!` negation.
///
/// `name` is lowercase, as returned by [`host_name`].
fn hosts_match(patterns: &HostPatterns, name: &str) -> bool {
    match patterns {
        HostPatterns::HashedName { salt, hash } => {
            let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, salt);
            hmac::verify(&key, name.as_bytes(), hash).is_ok()
        }
        HostPatterns::Patterns(patterns) => {
            let mut positive = false;
            for pattern in patterns {
                let (negated, pattern) = match pattern.strip_prefix('!') {
                    Some(pattern) => (true, pattern),
                    None => (false, pattern.as_str()),
                };
                if glob(&pattern.to_ascii_lowercase(), name) {
                    if negated {
                        return false;
                    }
                    positive = true;
                }
            }
            positive
        }
    }
}

/// Matches `text` against an OpenSSH glob (`*` any run, `?` one char).
fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut backtrack = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                backtrack = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match backtrack {
                Some((star, start)) => {
                    p = star + 1;
                    t = start + 1;
                    backtrack = Some((star, start + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Formats `key`'s SHA-256 fingerprint as `SHA256:<base64>`.
pub(crate) fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use russh::keys::{EcdsaCurve, HashAlg, load_public_key, load_secret_key};

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata").join(name)
    }

    fn ed25519() -> PublicKey {
        load_public_key(fixture("openssh_ed25519.pub")).expect("ed25519 fixture")
    }

    fn ed25519_other() -> PublicKey {
        load_public_key(fixture("openssh_ed25519_enc.pub")).expect("ed25519 fixture")
    }

    fn rsa() -> PublicKey {
        load_public_key(fixture("openssh_rsa.pub")).expect("rsa fixture")
    }

    fn ecdsa() -> PublicKey {
        load_secret_key(fixture("openssh_ecdsa"), None)
            .expect("ecdsa fixture")
            .public_key()
            .clone()
    }

    /// Renders `key` as `<type> <base64>`, independently of [`entry`].
    fn openssh(key: &PublicKey) -> String {
        PublicKey::new(key.key_data().clone(), "").to_openssh().expect("encode")
    }

    fn line(hosts: &str, key: &PublicKey) -> String {
        format!("{}\n", entry(hosts, key).expect("encode"))
    }

    fn check(text: &str, name: &str, key: &PublicKey) -> Check {
        KnownHosts::parse(text).check(name, key)
    }

    #[test]
    fn host_name_brackets_non_default_ports() {
        assert_eq!(host_name("Bastion.Example.com", 22), "bastion.example.com");
        assert_eq!(host_name("bastion", 2222), "[bastion]:2222");
        assert_eq!(host_name("::1", 2222), "[::1]:2222");
    }

    #[test]
    fn matches_plain_host() {
        let text = line("bastion", &ed25519());
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Match);
        assert_eq!(check(&text, "other", &ed25519()), Check::Unknown);
    }

    #[test]
    fn matches_hashed_entries() {
        // Generated with `ssh-keygen -H` for bastion.example.com and
        // [bastion.example.com]:2222.
        let key = openssh(&ed25519());
        let text = format!(
            "|1|YkYCGNXK4lNsQuU6n6A7tlBXMJA=|Fnik4ZJifxlQk5WIl8TORoabAYk= {key}\n\
             |1|gZLjgDBPMT3H7VMkuujL4JxjITI=|DHXtNa/vj7QO/fU7SB3Kd3J+bKA= {key}\n"
        );
        let known = KnownHosts::parse(&text);
        assert_eq!(known.check("bastion.example.com", &ed25519()), Check::Match);
        assert_eq!(known.check("[bastion.example.com]:2222", &ed25519()), Check::Match);
        assert_eq!(
            known.check(&host_name("BASTION.example.com", 22), &ed25519()),
            Check::Match
        );
        assert_eq!(known.check("[bastion.example.com]:2223", &ed25519()), Check::Unknown);
        assert_eq!(known.check("bastion", &ed25519()), Check::Unknown);
    }

    #[test]
    fn matches_bracketed_port_entries() {
        let text = line("[bastion]:2222", &ed25519());
        assert_eq!(check(&text, "[bastion]:2222", &ed25519()), Check::Match);
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Unknown);
        assert_eq!(check(&text, "[bastion]:22222", &ed25519()), Check::Unknown);
    }

    #[test]
    fn matches_any_host_in_comma_list() {
        let text = line("db1,bastion,10.0.0.5", &ed25519());
        for name in ["db1", "bastion", "10.0.0.5"] {
            assert_eq!(check(&text, name, &ed25519()), Check::Match, "{name}");
        }
        assert_eq!(check(&text, "db2", &ed25519()), Check::Unknown);
    }

    #[test]
    fn matches_wildcards() {
        let text = line("*.example.com,10.0.0.?,[jump-*]:2222", &ed25519());
        for name in ["a.example.com", "a.b.example.com", "10.0.0.7", "[jump-01]:2222"] {
            assert_eq!(check(&text, name, &ed25519()), Check::Match, "{name}");
        }
        for name in ["example.com", "10.0.0.10", "[jump-01]:22", "a.example.org"] {
            assert_eq!(check(&text, name, &ed25519()), Check::Unknown, "{name}");
        }
    }

    #[test]
    fn negation_excludes_host_even_when_positive_matches() {
        let text = line("*.example.com,!secret.example.com", &ed25519());
        assert_eq!(check(&text, "web.example.com", &ed25519()), Check::Match);
        assert_eq!(check(&text, "secret.example.com", &ed25519()), Check::Unknown);
        let only_negated = line("!secret.example.com", &ed25519());
        assert_eq!(check(&only_negated, "web.example.com", &ed25519()), Check::Unknown);
    }

    #[test]
    fn hostnames_match_case_insensitively() {
        let text = line("Bastion.EXAMPLE.com", &ed25519());
        assert_eq!(check(&text, "bastion.example.com", &ed25519()), Check::Match);
        assert_eq!(
            check(&text, &host_name("BASTION.example.COM", 22), &ed25519()),
            Check::Match
        );
    }

    #[test]
    fn revoked_key_wins_over_match() {
        let text = format!(
            "{}@revoked bastion {}\n",
            line("bastion", &ed25519()),
            openssh(&ed25519())
        );
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Revoked { line: 2 });
    }

    #[test]
    fn revoked_wildcard_applies_to_every_host() {
        let text = format!("@revoked * {}\n", openssh(&ed25519()));
        assert_eq!(check(&text, "anything", &ed25519()), Check::Revoked { line: 1 });
        assert_eq!(check(&text, "anything", &ed25519_other()), Check::Unknown);
    }

    #[test]
    fn rotation_accepts_any_recorded_key_of_the_type() {
        let text = format!("{}{}", line("bastion", &ed25519_other()), line("bastion", &ed25519()));
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Match);
        assert_eq!(check(&text, "bastion", &ed25519_other()), Check::Match);
    }

    #[test]
    fn different_key_of_same_type_is_changed_with_its_line() {
        let text = format!(
            "# comment\n\n{}{}",
            line("other", &ed25519()),
            line("bastion", &ed25519_other())
        );
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Changed { line: 4 });
    }

    #[test]
    fn key_of_unrecorded_type_for_known_host_is_changed() {
        let text = format!("{}{}", line("other", &ed25519()), line("bastion", &rsa()));
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Changed { line: 2 });
    }

    #[test]
    fn cert_authority_entries_are_ignored() {
        let text = format!("@cert-authority bastion {}\n", openssh(&ed25519()));
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Unknown);
        assert!(KnownHosts::parse(&text).algorithms("bastion").is_empty());
    }

    #[test]
    fn leading_whitespace_and_tabs_are_tolerated() {
        let text = format!("  \tbastion\t{}\n", openssh(&ed25519()).replacen(' ', "\t", 1));
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Match);
    }

    #[test]
    fn malformed_lines_are_skipped_and_line_numbers_stay_correct() {
        let text = format!(
            "garbage\nbastion ssh-ed25519 not-base64\n@bogus x {}\n{}",
            openssh(&ed25519()),
            line("bastion", &ed25519_other())
        );
        assert_eq!(check(&text, "bastion", &ed25519()), Check::Changed { line: 4 });
    }

    #[test]
    fn bom_and_crlf_are_stripped() {
        let text = format!(
            "\u{feff}{}\r\n{}\r\n",
            line("first", &ed25519()).trim_end(),
            line("bastion", &ed25519_other()).trim_end()
        );
        let known = KnownHosts::parse(&text);
        assert_eq!(known.check("first", &ed25519()), Check::Match);
        assert_eq!(known.check("bastion", &ed25519()), Check::Changed { line: 2 });
    }

    #[test]
    fn read_treats_missing_file_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let known = KnownHosts::read(&dir.path().join("missing")).expect("missing is empty");
        assert_eq!(known.check("bastion", &ed25519()), Check::Unknown);
    }

    #[test]
    fn algorithms_lists_recorded_types_for_host() {
        let text = format!(
            "{}{}{}@revoked bastion {}\n",
            line("bastion", &ecdsa()),
            line("bastion", &rsa()),
            line("other", &ed25519()),
            openssh(&ed25519())
        );
        let algorithms = KnownHosts::parse(&text).algorithms("bastion");
        assert_eq!(algorithms, vec![ecdsa().algorithm(), rsa().algorithm()]);
    }

    #[test]
    fn preferred_moves_recorded_algorithms_first() {
        let p256 = Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        };
        let sha512 = Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        };
        let sha256 = Algorithm::Rsa {
            hash: Some(HashAlg::Sha256),
        };
        let default = [Algorithm::Ed25519, p256.clone(), sha512.clone(), sha256.clone()];

        assert_eq!(preferred(&[], &default), default);
        assert_eq!(
            preferred(std::slice::from_ref(&p256), &default),
            vec![p256.clone(), Algorithm::Ed25519, sha512.clone(), sha256.clone()]
        );
        assert_eq!(
            preferred(&[Algorithm::Rsa { hash: None }], &default),
            vec![sha512, sha256, Algorithm::Ed25519, p256]
        );
    }

    #[test]
    fn append_creates_directory_and_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join(".ssh").join("known_hosts");

        append(&path, "[bastion]:2222", &ed25519()).expect("append");

        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(text, format!("[bastion]:2222 {}\n", openssh(&ed25519())));
        assert_eq!(
            KnownHosts::parse(&text).check("[bastion]:2222", &ed25519()),
            Check::Match
        );
    }

    #[test]
    fn append_adds_missing_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, "# existing").expect("seed");

        append(&path, "bastion", &ed25519()).expect("append");
        append(&path, "other", &rsa()).expect("append");

        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(
            text,
            format!(
                "# existing\nbastion {}\nother {}\n",
                openssh(&ed25519()),
                openssh(&rsa())
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn append_creates_private_directory_and_file() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let ssh_dir = dir.path().join(".ssh");
        let path = ssh_dir.join("known_hosts");

        append(&path, "bastion", &ed25519()).expect("append");

        let mode = |p: &Path| std::fs::metadata(p).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode(&ssh_dir), 0o700);
        assert_eq!(mode(&path), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn append_keeps_existing_file_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, "").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        append(&path, "bastion", &ed25519()).expect("append");

        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn fingerprint_is_sha256() {
        assert!(fingerprint(&ed25519()).starts_with("SHA256:"));
    }
}
