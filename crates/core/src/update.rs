//! Release discovery behind "Check for updates".
//!
//! This module parses GitHub's latest-release document, compares it with the
//! running version, decides when a startup check is due and keeps everything
//! the server sent at arm's length: release notes are reduced to plain text,
//! and only URLs on GitHub's own hosts are ever handed to a browser. It does no
//! I/O. The HTTPS transport lives in the platform crates, and nothing here is
//! used by the worker process.

use serde::Deserialize;
use std::fmt;

/// Repository whose releases are checked.
pub const REPOSITORY: &str = "jlobue10/Any2Odyssey";
/// GitHub REST endpoint for the newest published, non-prerelease release.
pub const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/jlobue10/Any2Odyssey/releases/latest";
/// Releases index, shown when a release has no usable page of its own.
pub const RELEASES_PAGE: &str = "https://github.com/jlobue10/Any2Odyssey/releases";
/// Largest response a transport may hand to [`parse_latest_release`].
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Release notes kept for display, in characters.
pub const MAX_NOTES_CHARS: usize = 4000;
/// Longest unrecognized tag echoed back in an error message, in characters.
pub const MAX_TAG_CHARS: usize = 64;
/// Longest JSON parser message kept in an error, in characters.
pub const MAX_ERROR_CHARS: usize = 200;
/// Automatic checks run at most this often.
pub const CHECK_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
/// Hosts whose HTTPS URLs may be opened from an update result.
pub const ALLOWED_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// A release version. Only plain `major.minor.patch` is recognized; pre-release
/// suffixes are rejected because the release workflow never publishes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// Accepts `0.8.2`, `v0.8.2` and surrounding whitespace, nothing else.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text
            .strip_prefix('v')
            .or_else(|| text.strip_prefix('V'))
            .unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u32>().ok()
        };
        let version = Version::new(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    /// The version this executable was built as. Checked at compile time, so
    /// a `0.9.0-rc1` in `Cargo.toml` fails the build instead of panicking in
    /// `--check-updates` and on the About page.
    pub const fn current() -> Version {
        CURRENT
    }

    /// The tag the release workflow creates for this version.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

/// `CARGO_PKG_VERSION` parsed at compile time; see [`Version::current`].
const CURRENT: Version = parse_release_version(crate::VERSION);

/// `const` parser for the package version: exactly `major.minor.patch` with
/// decimal digits. Any other shape is a compile error through the panics.
const fn parse_release_version(text: &str) -> Version {
    let bytes = text.as_bytes();
    let mut parts = [0u32; 3];
    let mut part = 0;
    let mut digits = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            assert!(
                digits > 0 && part < 2,
                "CARGO_PKG_VERSION must be major.minor.patch"
            );
            part += 1;
            digits = 0;
        } else {
            assert!(
                b.is_ascii_digit(),
                "CARGO_PKG_VERSION must be a plain release version"
            );
            // `Option::and_then` takes a closure and is not `const`; spell it out.
            let scaled = match parts[part].checked_mul(10) {
                Some(n) => n,
                None => panic!("CARGO_PKG_VERSION component overflows u32"),
            };
            parts[part] = match scaled.checked_add((b - b'0') as u32) {
                Some(n) => n,
                None => panic!("CARGO_PKG_VERSION component overflows u32"),
            };
            digits += 1;
        }
        i += 1;
    }
    assert!(
        part == 2 && digits > 0,
        "CARGO_PKG_VERSION must be major.minor.patch"
    );
    Version::new(parts[0], parts[1], parts[2])
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A published release, with every URL already checked by [`allowed_url`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// Release page, built from the parsed version rather than copied from the
    /// document so a surprising `html_url` can never reach the browser.
    pub page_url: String,
    /// The Windows x64 package, when the release carries one under the
    /// expected name on an allowed host. Like `page_url`, the URL is built
    /// from the version and the asset name rather than copied from the
    /// document, so nothing the server sent is ever opened.
    pub zip_url: Option<String>,
    /// The package's `.sha256` companion, under the same conditions.
    pub checksum_url: Option<String>,
    /// The Windows setup program (releases from 0.9.4 on), under the same
    /// conditions. It installs what the package's `Install.cmd` installs.
    pub setup_url: Option<String>,
    /// Release notes as plain text, see [`sanitize_notes`].
    pub notes: String,
    /// Publication time as the API reported it, kept only if it looks like a
    /// timestamp.
    pub published_at: Option<String>,
}

impl Release {
    pub fn package_name(version: Version) -> String {
        format!("Any2Odyssey-{version}-windows-x64.zip")
    }

    pub fn setup_name(version: Version) -> String {
        format!("Any2Odyssey-{version}-windows-x64-setup.exe")
    }

    /// Where GitHub serves `asset` of the release tagged for `version`.
    pub fn download_url(version: Version, asset: &str) -> String {
        format!("{RELEASES_PAGE}/download/{}/{asset}", version.tag())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    TooLarge(usize),
    Json(String),
    Draft,
    Prerelease,
    MissingTag,
    /// The tag is not a version number at all.
    UnrecognizedTag(String),
    /// The tag parses as a version but is not spelled the way the release
    /// workflow tags and the download links are built (`v<major>.<minor>.<patch>`,
    /// no capital `V`, no leading zeros), so every synthesized link would 404.
    TagMismatch {
        tag: String,
        expected: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::TooLarge(bytes) => write!(
                f,
                "release information is too large ({bytes} bytes, limit {MAX_RESPONSE_BYTES})"
            ),
            ParseError::Json(message) => write!(f, "release information is not valid: {message}"),
            ParseError::Draft => write!(f, "the latest release is a draft"),
            ParseError::Prerelease => write!(f, "the latest release is a pre-release"),
            ParseError::MissingTag => write!(f, "the latest release has no tag"),
            ParseError::UnrecognizedTag(tag) => {
                write!(f, "the release tag {tag:?} is not a version number")
            }
            ParseError::TagMismatch { tag, expected } => write!(
                f,
                "the release tag {tag:?} is not spelled {expected:?}, so its download links would not resolve"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Deserialize)]
struct Document {
    tag_name: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// One release asset. Both fields are optional so one odd entry in the list
/// cannot fail the whole check; an asset missing either is simply not offered.
#[derive(Deserialize)]
struct Asset {
    name: Option<String>,
    browser_download_url: Option<String>,
}

/// Parse the JSON body of [`LATEST_RELEASE_API`]. Unknown fields are ignored so
/// GitHub can extend the document; missing ones degrade to `None`.
pub fn parse_latest_release(body: &str) -> Result<Release, ParseError> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err(ParseError::TooLarge(body.len()));
    }
    // serde quotes an unexpected string in full ("invalid type: string
    // \"...\", expected a boolean"), so the message can be as long as the
    // response; it is shown on the About page and written to the log.
    let document: Document = serde_json::from_str(body)
        .map_err(|e| ParseError::Json(bounded(&e.to_string(), MAX_ERROR_CHARS)))?;
    if document.draft {
        return Err(ParseError::Draft);
    }
    if document.prerelease {
        return Err(ParseError::Prerelease);
    }
    let tag = document.tag_name.ok_or(ParseError::MissingTag)?;
    // Every URL below is rebuilt from the version as `v<major>.<minor>.<patch>`,
    // and GitHub's tag paths are literal, so a tag spelled any other way
    // (`V0.9.0`, `v0.09.0`) would 404 even though the version parses.
    let version = Version::parse(&tag)
        .ok_or_else(|| ParseError::UnrecognizedTag(bounded(&tag, MAX_TAG_CHARS)))?;
    if version.tag() != tag.trim() {
        return Err(ParseError::TagMismatch {
            tag: bounded(&tag, MAX_TAG_CHARS),
            expected: version.tag(),
        });
    }
    // The document only confirms that the asset exists under its expected
    // name on an allowed host; the URL handed to the browser is synthesized.
    let asset_url = |name: &str| {
        document
            .assets
            .iter()
            .any(|asset| {
                asset.name.as_deref() == Some(name)
                    && asset
                        .browser_download_url
                        .as_deref()
                        .is_some_and(allowed_url)
            })
            .then(|| Release::download_url(version, name))
    };
    let package = Release::package_name(version);
    Ok(Release {
        version,
        page_url: format!("{RELEASES_PAGE}/tag/{}", version.tag()),
        zip_url: asset_url(&package),
        checksum_url: asset_url(&format!("{package}.sha256")),
        setup_url: asset_url(&Release::setup_name(version)),
        notes: sanitize_notes(document.body.as_deref().unwrap_or_default()),
        published_at: document
            .published_at
            .filter(|text| looks_like_timestamp(text)),
    })
}

/// The first `max` characters of `text`, with an ellipsis when it was cut.
fn bounded(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().nth(max).is_some() {
        out.push('…');
    }
    out
}

fn looks_like_timestamp(text: &str) -> bool {
    text.len() <= 40
        && text.len() >= 10
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':' | b'T' | b'Z' | b'+' | b'.'))
}

/// True for an `https://` URL whose authority is exactly one of
/// [`ALLOWED_HOSTS`]: no user information, no port, no other scheme.
pub fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains(['@', ':', '\\']) {
        return false;
    }
    let host = authority.to_ascii_lowercase();
    ALLOWED_HOSTS.contains(&host.as_str())
}

/// Reduce release notes to plain text: drop control characters and the
/// invisible formatting characters listed in [`is_invisible_formatting`],
/// turn every kind of line break into `\n`, collapse blank runs and cap the
/// length. Markdown is left as-is; it is displayed, not rendered.
pub fn sanitize_notes(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_NOTES_CHARS + 1));
    let mut newlines = 0usize;
    let mut chars = 0usize;
    let mut truncated = false;
    let mut source = text.chars().peekable();
    while let Some(c) = source.next() {
        let c = match c {
            // CRLF is one break, written by the '\n' that follows.
            '\r' if source.peek() == Some(&'\n') => continue,
            '\t' => ' ',
            // Every other character that ends a line somewhere is a break
            // here too, since the display only breaks at '\n': a lone
            // carriage return, vertical tab, form feed, next line (NEL) and
            // Unicode's line and paragraph separators. Dropping them, as the
            // control-character rule below would, runs two lines together.
            '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => '\n',
            // Narrow no-break space has width (French punctuation, digit
            // grouping); dropping it would run words together.
            '\u{202F}' => ' ',
            c if c.is_control() || is_invisible_formatting(c) => continue,
            c => c,
        };
        if c == '\n' {
            newlines += 1;
            if newlines > 2 {
                continue;
            }
        } else {
            newlines = 0;
        }
        if chars == MAX_NOTES_CHARS {
            truncated = true;
            break;
        }
        out.push(c);
        chars += 1;
    }
    let mut out = out.trim().to_owned();
    if truncated {
        out.push('…');
    }
    out
}

/// Characters that render as nothing but change how surrounding text is read
/// or copied: every `Bidi_Control` character (explicit embeddings, overrides
/// and isolates, the marks, the Arabic letter mark), zero-width space, the
/// word joiner and invisible operators, the deprecated format controls
/// U+206A to U+206F (symmetric swapping, Arabic form shaping, digit shapes),
/// soft hyphen, Mongolian vowel separator, interlinear annotation controls
/// and the tag characters used to hide text.
///
/// Kept on purpose, although invisible: the zero-width joiner and non-joiner
/// (U+200C, U+200D), which emoji sequences and Arabic, Persian and Indic
/// shaping depend on, and the variation selectors (U+FE00 to U+FE0F, U+E0100
/// to U+E01EF), which choose emoji presentation and the ideograph variants
/// used in names; they alter one glyph, not how the text around it reads.
/// The line and paragraph separators and the narrow no-break space are
/// visible and are mapped in [`sanitize_notes`] rather than listed here.
fn is_invisible_formatting(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{206A}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// How a published release relates to the running version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// The running version is the published one.
    UpToDate,
    /// A newer release exists.
    Available(Release),
    /// The running version is newer than anything published: a development
    /// or pre-release build. Not an update.
    AheadOfRelease,
}

pub fn compare(current: Version, release: Release) -> Comparison {
    match release.version.cmp(&current) {
        std::cmp::Ordering::Greater => Comparison::Available(release),
        std::cmp::Ordering::Equal => Comparison::UpToDate,
        std::cmp::Ordering::Less => Comparison::AheadOfRelease,
    }
}

/// Whether an automatic check should run now. `last_check` and `now` are Unix
/// seconds. A missing record or a clock that moved backwards both mean "check".
pub fn check_due(last_check: Option<i64>, now: i64) -> bool {
    match last_check {
        None => true,
        Some(last) if last > now => true,
        Some(last) => now.saturating_sub(last) >= CHECK_INTERVAL_SECONDS,
    }
}

/// What a transport sends. Kept here so the headers are visible and tested;
/// nothing identifies the machine beyond the standard client name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub url: &'static str,
    pub user_agent: String,
    pub accept: &'static str,
    pub api_version: &'static str,
}

impl Request {
    pub fn latest_release() -> Request {
        Request {
            url: LATEST_RELEASE_API,
            user_agent: format!("{}/{}", crate::APP_NAME, crate::VERSION),
            accept: "application/vnd.github+json",
            api_version: "2022-11-28",
        }
    }
}

/// Seconds since the Unix epoch, for [`check_due`] and the saved last check.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What the controls know about updates, shown on the About page and in the
/// tray tooltip.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Unchecked,
    Checking,
    UpToDate,
    AheadOfRelease,
    Available(Release),
    Failed(String),
}

impl UpdateState {
    pub fn from_result(result: Result<Comparison, String>) -> UpdateState {
        match result {
            Ok(Comparison::UpToDate) => UpdateState::UpToDate,
            Ok(Comparison::AheadOfRelease) => UpdateState::AheadOfRelease,
            Ok(Comparison::Available(release)) => UpdateState::Available(release),
            Err(error) => UpdateState::Failed(error),
        }
    }

    pub fn available(&self) -> Option<&Release> {
        match self {
            UpdateState::Available(release) => Some(release),
            _ => None,
        }
    }

    /// A check that produced an answer, successful or not.
    pub fn is_settled(&self) -> bool {
        !matches!(self, UpdateState::Unchecked | UpdateState::Checking)
    }

    pub fn summary(&self) -> String {
        let running = crate::VERSION;
        match self {
            UpdateState::Unchecked => "Not checked yet.".to_owned(),
            UpdateState::Checking => "Checking GitHub for the newest release…".to_owned(),
            UpdateState::UpToDate => {
                format!("{} {running} is the newest release.", crate::APP_NAME)
            }
            UpdateState::AheadOfRelease => {
                format!("Version {running} is newer than the newest published release.")
            }
            UpdateState::Available(release) => format!(
                "Version {} is available; this is {running}.",
                release.version
            ),
            UpdateState::Failed(error) => format!("Could not check for updates: {error}"),
        }
    }
}

/// Parse a latest-release document and relate it to the running version.
pub fn evaluate_latest_release(body: &str) -> Result<Comparison, ParseError> {
    parse_latest_release(body).map(|release| compare(Version::current(), release))
}

/// Exit codes of `Any2Odyssey --check-updates`, for scripts and tests.
pub mod exit_code {
    pub const UP_TO_DATE: i32 = 0;
    pub const CHECK_FAILED: i32 = 1;
    pub const UPDATE_AVAILABLE: i32 = 10;
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "url": "https://api.github.com/repos/jlobue10/Any2Odyssey/releases/1",
      "html_url": "https://github.com/jlobue10/Any2Odyssey/releases/tag/v0.9.0",
      "tag_name": "v0.9.0",
      "name": "0.9.0",
      "draft": false,
      "prerelease": false,
      "published_at": "2026-12-01T18:04:31Z",
      "author": { "login": "jlobue10", "id": 1 },
      "assets": [
        { "name": "Any2Odyssey-0.9.0-windows-x64.zip",
          "browser_download_url": "https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.0/Any2Odyssey-0.9.0-windows-x64.zip",
          "size": 3909802 },
        { "name": "Any2Odyssey-0.9.0-windows-x64.zip.sha256",
          "browser_download_url": "https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.0/Any2Odyssey-0.9.0-windows-x64.zip.sha256" }
      ],
      "body": "Windows x64 package.\r\n\r\n\r\n\r\n## What's Changed\r\n* Fix\u0007 thing\u202E by @jlobue10"
    }"#;

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(Version::parse("0.8.2"), Some(Version::new(0, 8, 2)));
        assert_eq!(Version::parse(" v1.10.0\n"), Some(Version::new(1, 10, 0)));
        assert_eq!(Version::parse("V0.8.2"), Some(Version::new(0, 8, 2)));
        for invalid in [
            "",
            "v",
            "0.8",
            "0.8.2.1",
            "0.8.2-rc1",
            "0.8.2 beta",
            "0.-8.2",
            "a.b.c",
            "0..2",
            "v 0.8.2",
            "99999999999.0.0",
            "0.8.2\0",
        ] {
            assert_eq!(Version::parse(invalid), None, "{invalid:?}");
        }
        assert!(Version::new(0, 9, 0) > Version::new(0, 8, 12));
        assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
        assert!(Version::new(0, 8, 2) > Version::new(0, 8, 1));
        assert_eq!(Version::new(0, 8, 2).to_string(), "0.8.2");
        assert_eq!(Version::new(0, 8, 2).tag(), "v0.8.2");
    }

    #[test]
    fn current_version_is_the_package_version() {
        assert_eq!(Version::current().to_string(), crate::VERSION);
    }

    #[test]
    fn sample_document_yields_a_release() {
        let release = parse_latest_release(SAMPLE).unwrap();
        assert_eq!(release.version, Version::new(0, 9, 0));
        assert_eq!(
            release.page_url,
            "https://github.com/jlobue10/Any2Odyssey/releases/tag/v0.9.0"
        );
        assert_eq!(
            release.zip_url.as_deref(),
            Some("https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.0/Any2Odyssey-0.9.0-windows-x64.zip")
        );
        assert!(release
            .checksum_url
            .as_deref()
            .unwrap()
            .ends_with(".zip.sha256"));
        assert_eq!(
            release.published_at.as_deref(),
            Some("2026-12-01T18:04:31Z")
        );
        assert_eq!(
            release.notes,
            "Windows x64 package.\n\n## What's Changed\n* Fix thing by @jlobue10"
        );
    }

    #[test]
    fn setup_program_is_offered_only_when_the_release_has_one() {
        // The sample is a release from before the setup program existed.
        assert_eq!(parse_latest_release(SAMPLE).unwrap().setup_url, None);
        let body = r#"{"tag_name":"v0.9.4","assets":[
            {"name":"Any2Odyssey-0.9.4-windows-x64.zip","browser_download_url":"https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.4/Any2Odyssey-0.9.4-windows-x64.zip"},
            {"name":"Any2Odyssey-0.9.4-windows-x64-setup.exe","browser_download_url":"https://github.com/someone-else/other-repo/releases/download/v1/setup.exe"}
        ]}"#;
        let release = parse_latest_release(body).unwrap();
        // Synthesized from the version, like the package's: never the URL the document carried.
        assert_eq!(
            release.setup_url.as_deref(),
            Some("https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.4/Any2Odyssey-0.9.4-windows-x64-setup.exe")
        );
        assert!(allowed_url(release.setup_url.as_deref().unwrap()));
        assert!(release.zip_url.is_some());
        // Another release's setup program, a differently spelled one, one on a host that is not allowed.
        let body = r#"{"tag_name":"v0.9.4","assets":[
            {"name":"Any2Odyssey-0.9.3-windows-x64-setup.exe","browser_download_url":"https://github.com/a.exe"},
            {"name":"any2odyssey-0.9.4-windows-x64-setup.exe","browser_download_url":"https://github.com/b.exe"},
            {"name":"Any2Odyssey-0.9.4-windows-x64-setup.exe","browser_download_url":"https://github.com.evil.example/c.exe"}
        ]}"#;
        assert_eq!(parse_latest_release(body).unwrap().setup_url, None);
    }

    #[test]
    fn page_url_comes_from_the_tag_not_the_document() {
        let body = r#"{"tag_name":"v0.9.1","html_url":"https://evil.example/x"}"#;
        let release = parse_latest_release(body).unwrap();
        assert_eq!(
            release.page_url,
            "https://github.com/jlobue10/Any2Odyssey/releases/tag/v0.9.1"
        );
        assert_eq!(release.zip_url, None);
        assert_eq!(release.checksum_url, None);
        assert_eq!(release.notes, "");
        assert_eq!(release.published_at, None);
    }

    #[test]
    fn asset_urls_are_synthesized_from_the_version_not_copied() {
        // The document vouches for the asset's existence on an allowed host;
        // a surprising path under that host is still never opened.
        let body = r#"{"tag_name":"v0.9.0","assets":[
            {"name":"Any2Odyssey-0.9.0-windows-x64.zip","browser_download_url":"https://github.com/someone-else/other-repo/releases/download/v1/x.zip"},
            {"name":"Any2Odyssey-0.9.0-windows-x64.zip.sha256","browser_download_url":"https://objects.githubusercontent.com/anything"}
        ]}"#;
        let release = parse_latest_release(body).unwrap();
        assert_eq!(
            release.zip_url.as_deref(),
            Some("https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.0/Any2Odyssey-0.9.0-windows-x64.zip")
        );
        assert_eq!(
            release.checksum_url.as_deref(),
            Some("https://github.com/jlobue10/Any2Odyssey/releases/download/v0.9.0/Any2Odyssey-0.9.0-windows-x64.zip.sha256")
        );
        assert!(allowed_url(release.zip_url.as_deref().unwrap()));
    }

    #[test]
    fn assets_on_other_hosts_or_with_other_names_are_ignored() {
        let body = r#"{"tag_name":"v0.9.0","assets":[
            {"name":"Any2Odyssey-0.9.0-windows-x64.zip","browser_download_url":"https://github.com.evil.example/a.zip"},
            {"name":"Any2Odyssey-0.9.0-windows-x64.zip","browser_download_url":"http://github.com/a.zip"},
            {"name":"Any2Odyssey-0.8.0-windows-x64.zip","browser_download_url":"https://github.com/old.zip"},
            {"name":"any2odyssey-0.9.0-windows-x64.zip","browser_download_url":"https://github.com/case.zip"}
        ]}"#;
        let release = parse_latest_release(body).unwrap();
        assert_eq!(release.zip_url, None);
        assert_eq!(release.checksum_url, None);
    }

    #[test]
    fn documents_that_are_not_a_release_are_rejected() {
        assert_eq!(
            parse_latest_release(r#"{"tag_name":"v0.9.0","draft":true}"#),
            Err(ParseError::Draft)
        );
        assert_eq!(
            parse_latest_release(r#"{"tag_name":"v0.9.0","prerelease":true}"#),
            Err(ParseError::Prerelease)
        );
        assert_eq!(
            parse_latest_release(r#"{"name":"0.9.0"}"#),
            Err(ParseError::MissingTag)
        );
        assert_eq!(
            parse_latest_release(r#"{"tag_name":"nightly"}"#),
            Err(ParseError::UnrecognizedTag("nightly".into()))
        );
        // Parses as a version, but the synthesized download URLs would not
        // match it; the error names the spelling they are built from.
        for tag in ["V0.9.0", "v0.09.0", "0.9.0"] {
            let error = parse_latest_release(&format!(r#"{{"tag_name":"{tag}"}}"#)).unwrap_err();
            assert_eq!(
                error,
                ParseError::TagMismatch {
                    tag: tag.into(),
                    expected: "v0.9.0".into(),
                },
                "{tag}"
            );
            let shown = error.to_string();
            assert!(
                shown.contains("\"v0.9.0\"") && !shown.contains("not a version number"),
                "{shown}"
            );
        }
        assert!(matches!(
            parse_latest_release("[]"),
            Err(ParseError::Json(_))
        ));
        assert!(matches!(
            parse_latest_release("not json"),
            Err(ParseError::Json(_))
        ));
        // One malformed asset entry does not fail the check; it is just not offered.
        let release =
            parse_latest_release(r#"{"tag_name":"v0.9.0","assets":[{"name":"x"},{"size":1}]}"#)
                .unwrap();
        assert_eq!(release.zip_url, None);
        let long_tag = "x".repeat(500);
        match parse_latest_release(&format!(r#"{{"tag_name":"{long_tag}"}}"#)) {
            Err(ParseError::UnrecognizedTag(shown)) => {
                assert_eq!(shown.chars().count(), MAX_TAG_CHARS + 1);
                assert!(shown.ends_with('…'));
            }
            other => panic!("{other:?}"),
        }
        let huge = format!(
            r#"{{"tag_name":"v0.9.0","body":"{}"}}"#,
            "x".repeat(MAX_RESPONSE_BYTES)
        );
        assert_eq!(
            parse_latest_release(&huge),
            Err(ParseError::TooLarge(huge.len()))
        );
        // An odd publication time is dropped, not trusted.
        let release =
            parse_latest_release(r#"{"tag_name":"v0.9.0","published_at":"<b>soon</b>"}"#).unwrap();
        assert_eq!(release.published_at, None);
    }

    #[test]
    fn a_parser_message_that_quotes_the_response_is_bounded() {
        // Found by fuzzing: a response that is one JSON string, or that
        // carries a long string where a flag belongs, came back whole inside
        // the error text (megabytes at the response limit).
        for body in [
            format!("\"{}\"", "\u{7f}".repeat(100_000)),
            format!(
                r#"{{"tag_name":"v0.9.0","draft":"{}"}}"#,
                "x".repeat(100_000)
            ),
        ] {
            match parse_latest_release(&body) {
                Err(ParseError::Json(message)) => {
                    assert!(message.starts_with("invalid type: string"), "{message}");
                    assert_eq!(message.chars().count(), MAX_ERROR_CHARS + 1);
                    assert!(message.ends_with('…'));
                }
                other => panic!("{other:?}"),
            }
        }
        // An ordinary message is kept whole, position included.
        match parse_latest_release("not json") {
            Err(ParseError::Json(message)) => {
                assert!(message.contains(" at line 1 column "), "{message}");
                assert!(!message.ends_with('…'), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_https_urls_on_github_hosts_are_allowed() {
        for ok in [
            "https://github.com/jlobue10/Any2Odyssey/releases",
            "https://GitHub.com/x",
            "https://github.com",
            "https://github.com?x=1",
            "https://objects.githubusercontent.com/github-production-release-asset/1",
            "https://release-assets.githubusercontent.com/x",
        ] {
            assert!(allowed_url(ok), "{ok}");
        }
        for bad in [
            "",
            "github.com",
            "http://github.com/x",
            "https://github.com.evil.example/x",
            "https://evil.example/github.com/x",
            "https://user@github.com/x",
            "https://github.com:443/x",
            "https://github.com\\evil.example/x",
            "javascript:alert(1)",
            "file:///C:/Windows",
            "https://api.github.com/repos",
            "https://githubusercontent.com/x",
            "https:///x",
        ] {
            assert!(!allowed_url(bad), "{bad}");
        }
    }

    #[test]
    fn notes_become_bounded_plain_text() {
        assert_eq!(
            sanitize_notes("  a\r\n\r\n\r\n\r\nb\t\tc \u{1b}[31m  "),
            "a\n\nb  c [31m"
        );
        assert_eq!(sanitize_notes("\u{202E}x\u{200B}y\u{FEFF}z\u{2066}"), "xyz");
        assert_eq!(sanitize_notes("\u{7f}\u{0}\u{1}"), "");
        // Every Bidi_Control character, hidden-text tag characters and the
        // soft hyphen vanish too.
        assert_eq!(
            sanitize_notes(
                "a\u{061C}b\u{200E}c\u{200F}d\u{202A}e\u{202B}f\u{202C}g\u{202D}h\u{2067}i\u{2068}j\u{2069}k"
            ),
            "abcdefghijk"
        );
        assert_eq!(
            sanitize_notes("safe\u{E0001}\u{E0068}\u{E0069}\u{E007F} text\u{00AD}\u{FFF9}x\u{FFFA}y\u{FFFB}\u{180E}"),
            "safe textxy"
        );
        // Visible separators keep their meaning: line and paragraph separators
        // become line breaks, the narrow no-break space a plain space.
        assert_eq!(
            sanitize_notes("one\u{2028}two\u{2029}three\u{202F}four 1\u{202F}000"),
            "one\ntwo\nthree four 1 000"
        );
        assert_eq!(sanitize_notes("a\u{2029}\u{2029}\u{2029}b"), "a\n\nb");
        // So does every other line terminator; CRLF stays a single break.
        assert_eq!(
            sanitize_notes("mac\rnel\u{85}vt\u{b}ff\u{c}crlf\r\nend"),
            "mac\nnel\nvt\nff\ncrlf\nend"
        );
        assert_eq!(sanitize_notes("a\r\rb\r\n\r\n\r\nc\r"), "a\n\nb\n\nc");
        // Deprecated format controls vanish; variation selectors, which only
        // pick a glyph, stay with the character they follow.
        assert_eq!(
            sanitize_notes("x\u{206A}\u{206B}\u{206C}\u{206D}\u{206E}\u{206F}y"),
            "xy"
        );
        assert_eq!(
            sanitize_notes("\u{2764}\u{FE0F} \u{845B}\u{E0100}"),
            "\u{2764}\u{FE0F} \u{845B}\u{E0100}"
        );
        // Joiners stay: emoji sequences and cursive scripts need them.
        assert_eq!(
            sanitize_notes("\u{1F469}\u{200D}\u{1F4BB} \u{0646}\u{200C}\u{0627}"),
            "\u{1F469}\u{200D}\u{1F4BB} \u{0646}\u{200C}\u{0627}"
        );
        let long = "é".repeat(MAX_NOTES_CHARS + 10);
        let notes = sanitize_notes(&long);
        assert_eq!(notes.chars().count(), MAX_NOTES_CHARS + 1);
        assert!(notes.ends_with('…'));
        let exact = "x".repeat(MAX_NOTES_CHARS);
        assert_eq!(sanitize_notes(&exact), exact);
    }

    #[test]
    fn comparison_distinguishes_update_current_and_development_builds() {
        let release = parse_latest_release(SAMPLE).unwrap();
        assert_eq!(
            compare(Version::new(0, 8, 1), release.clone()),
            Comparison::Available(release.clone())
        );
        assert_eq!(
            compare(Version::new(0, 9, 0), release.clone()),
            Comparison::UpToDate
        );
        assert_eq!(
            compare(Version::new(0, 9, 1), release),
            Comparison::AheadOfRelease
        );
    }

    #[test]
    fn automatic_checks_are_daily_and_tolerate_clock_changes() {
        let now = 1_800_000_000;
        assert!(check_due(None, now));
        assert!(!check_due(Some(now), now));
        assert!(!check_due(Some(now - CHECK_INTERVAL_SECONDS + 1), now));
        assert!(check_due(Some(now - CHECK_INTERVAL_SECONDS), now));
        assert!(check_due(Some(now + 60), now));
    }

    #[test]
    fn automatic_checks_accept_the_full_saved_timestamp_range() {
        let saved: crate::prefs::UiSettings =
            serde_json::from_str(r#"{"LastUpdateCheck":-9223372036854775808}"#).unwrap();
        assert!(check_due(saved.last_update_check, 1_800_000_000));
        let times = [
            i64::MIN,
            -CHECK_INTERVAL_SECONDS,
            -1,
            0,
            CHECK_INTERVAL_SECONDS,
            i64::MAX,
        ];
        for now in times {
            for last in times {
                let due = last > now
                    || i128::from(now) - i128::from(last) >= i128::from(CHECK_INTERVAL_SECONDS);
                assert_eq!(check_due(Some(last), now), due, "last={last}, now={now}");
            }
        }
    }

    #[test]
    fn request_targets_the_api_with_a_plain_client_name() {
        let request = Request::latest_release();
        assert_eq!(request.url, LATEST_RELEASE_API);
        assert!(request.url.starts_with("https://api.github.com/repos/"));
        assert!(request.url.contains(REPOSITORY));
        assert_eq!(
            request.user_agent,
            format!("Any2Odyssey/{}", crate::VERSION)
        );
        assert_eq!(request.accept, "application/vnd.github+json");
        assert!(request.user_agent.bytes().all(|b| b.is_ascii_graphic()));
    }

    #[test]
    fn a_signed_number_is_not_a_version_component() {
        // `u32::from_str` takes a leading plus sign; a version does not.
        for invalid in ["+0.8.2", "0.+8.2", "0.8.+2", "v+1.0.0"] {
            assert_eq!(Version::parse(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn a_response_of_exactly_the_limit_is_read_and_one_byte_more_is_not() {
        let document = |length: usize| {
            let empty = r#"{"tag_name":"v0.9.0","body":""}"#;
            let body = format!(
                r#"{{"tag_name":"v0.9.0","body":"{}"}}"#,
                "x".repeat(length - empty.len())
            );
            assert_eq!(body.len(), length);
            body
        };
        let release = parse_latest_release(&document(MAX_RESPONSE_BYTES)).unwrap();
        assert_eq!(release.version, Version::new(0, 9, 0));
        assert_eq!(
            parse_latest_release(&document(MAX_RESPONSE_BYTES + 1)),
            Err(ParseError::TooLarge(MAX_RESPONSE_BYTES + 1))
        );
    }

    #[test]
    fn automatic_checks_are_a_day_apart_by_the_real_clock() {
        // Every other test names the interval by its constant.
        assert_eq!(CHECK_INTERVAL_SECONDS, 86_400);
        // 2023-11-14: a clock that reads zero would make every start due.
        assert!(now_unix() > 1_700_000_000, "{}", now_unix());
    }
}

#[cfg(test)]
mod evaluate_tests {
    use super::*;

    #[test]
    fn evaluation_relates_the_document_to_the_running_version() {
        let current = Version::current();
        let future = Version::new(current.major + 1, 0, 0);
        let body = format!(r#"{{"tag_name":"{}"}}"#, future.tag());
        match evaluate_latest_release(&body) {
            Ok(Comparison::Available(release)) => assert_eq!(release.version, future),
            other => panic!("{other:?}"),
        }
        let body = format!(r#"{{"tag_name":"{}"}}"#, current.tag());
        assert_eq!(evaluate_latest_release(&body), Ok(Comparison::UpToDate));
        let body = r#"{"tag_name":"v0.0.1"}"#;
        assert_eq!(
            evaluate_latest_release(body),
            Ok(Comparison::AheadOfRelease)
        );
        assert_eq!(
            evaluate_latest_release(r#"{"tag_name":"v1.0.0","draft":true}"#),
            Err(ParseError::Draft)
        );
        assert_ne!(exit_code::UPDATE_AVAILABLE, exit_code::UP_TO_DATE);
        assert_ne!(exit_code::UPDATE_AVAILABLE, exit_code::CHECK_FAILED);
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn update_state_reflects_each_outcome() {
        let release = parse_latest_release(r#"{"tag_name":"v99.0.0"}"#).unwrap();
        let available = UpdateState::from_result(Ok(Comparison::Available(release.clone())));
        assert_eq!(available.available(), Some(&release));
        assert!(available.summary().contains("99.0.0"));
        assert!(available.summary().contains(crate::VERSION));
        assert!(available.is_settled());
        let current = UpdateState::from_result(Ok(Comparison::UpToDate));
        assert_eq!(current, UpdateState::UpToDate);
        assert!(current.summary().contains(crate::VERSION));
        assert_eq!(
            UpdateState::from_result(Ok(Comparison::AheadOfRelease)),
            UpdateState::AheadOfRelease
        );
        let failed = UpdateState::from_result(Err("offline".into()));
        assert_eq!(failed, UpdateState::Failed("offline".into()));
        assert!(failed.summary().contains("offline"));
        assert!(failed.available().is_none());
        assert!(!UpdateState::default().is_settled());
        assert!(!UpdateState::Checking.is_settled());
    }
}
