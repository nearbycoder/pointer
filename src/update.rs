//! Updates from GitHub releases. The app reads the repository's latest release, and when its
//! version is newer, downloads the release's `Pointer.zip`, checks that the app inside is
//! signed by the same Apple team as the running one, swaps it in place of the running bundle
//! and relaunches. Everything goes through the system's own `curl`, `ditto` and `codesign`.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = "nearbycoder/pointer";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Every release carries the app under this name, so the latest is always at
/// `releases/latest/download/Pointer.zip`.
pub const ASSET: &str = "Pointer.zip";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    /// The zip to download.
    pub url: String,
    /// The release's page on GitHub.
    pub page: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Unchecked,
    Checking,
    Current,
    Available(Release),
    /// Downloading, checking or swapping: what it's doing now.
    Installing(Release, &'static str),
    Failed(String),
}

/// The app bundle this process runs from, if it runs from one.
pub fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension().is_some_and(|e| e == "app") && exe.parent()?.ends_with("Contents/MacOS")).then(|| app.to_path_buf())
}

pub fn bundled() -> bool {
    bundle().is_some()
}

fn run(cmd: &str, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new(cmd).args(args).output().map_err(|e| format!("couldn't run {cmd}: {e}"))
}

fn curl(args: &[&str]) -> Result<Vec<u8>, String> {
    let mut all = vec!["-fsSL", "--max-time", "120", "-H", "User-Agent: Pointer-updater"];
    all.extend_from_slice(args);
    let out = run("/usr/bin/curl", &all)?;
    if out.status.success() { Ok(out.stdout) } else { Err(format!("download failed: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

/// "1.2.10" as numbers, for comparing; a leading "v" and any "-suffix" are ignored.
fn parse(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y { return x > y; }
    }
    false
}

/// The version releases are compared with: this build's, unless `POINTER_UPDATE_AS` stands
/// in for an older one (to try an update without building an older app).
fn current() -> String {
    std::env::var("POINTER_UPDATE_AS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| VERSION.to_string())
}

/// The latest release on GitHub, if it's newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = curl(&["-H", "Accept: application/vnd.github+json", &url])?;
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| format!("GitHub's answer didn't parse: {e}"))?;
    let tag = json["tag_name"].as_str().ok_or("the latest release has no tag")?;
    if !newer(tag, &current()) { return Ok(None); }
    let asset = json["assets"].as_array().and_then(|assets| {
        assets.iter().find(|a| a["name"].as_str() == Some(ASSET)).and_then(|a| a["browser_download_url"].as_str())
    });
    let Some(asset) = asset else { return Err(format!("release {tag} has no {ASSET}")) };
    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: json["body"].as_str().unwrap_or("").trim().to_string(),
        url: asset.to_string(),
        page: json["html_url"].as_str().unwrap_or("").to_string(),
    }))
}

/// The Apple team that signed `path`, if any.
fn team(path: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/codesign").args(["-dv", "--verbose=2"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    text.lines().find_map(|l| l.strip_prefix("TeamIdentifier=")).map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && t != "not set")
}

fn short_version(app: &Path) -> Option<String> {
    let out = run("/usr/bin/plutil", &["-extract", "CFBundleShortVersionString", "raw", "-o", "-", &app.join("Contents/Info.plist").to_string_lossy()]).ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Whether this process can put a new app beside the old one (not on the disk image, say).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".pointer-update-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Downloads, checks and swaps in `release`. Returns the bundle to relaunch.
pub fn install(release: &Release, step: impl Fn(&'static str)) -> Result<PathBuf, String> {
    let current = bundle().ok_or("Updates install into the Pointer app; this is a development build")?;
    let ours = team(&current).ok_or("This copy of Pointer isn't signed, so it can't check an update's signature")?;
    let parent = current.parent().ok_or("the app has no folder")?;
    if current.to_string_lossy().contains("/AppTranslocation/") || !writable(parent) {
        return Err("Pointer can't replace itself where it is. Drag it into Applications, open it from there, then update.".into());
    }
    let work = std::env::temp_dir().join(format!("pointer-update-{}-{}", release.version, std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("couldn't make a work folder: {e}"))?;
    let zip = work.join(ASSET);
    step("Downloading");
    curl(&["-o", &zip.to_string_lossy(), &release.url])?;
    step("Unpacking");
    let unpacked = work.join("unpacked");
    let out = run("/usr/bin/ditto", &["-x", "-k", &zip.to_string_lossy(), &unpacked.to_string_lossy()])?;
    if !out.status.success() { return Err("the download didn't unpack".into()); }
    let fresh = unpacked.join("Pointer.app");
    if !fresh.is_dir() { return Err("the download has no Pointer.app".into()); }
    step("Checking the signature");
    let verify = run("/usr/bin/codesign", &["--verify", "--deep", "--strict", &fresh.to_string_lossy()])?;
    if !verify.status.success() { return Err("the update's signature doesn't check out".into()); }
    if team(&fresh).as_deref() != Some(ours.as_str()) { return Err("the update isn't signed by Pointer's developer".into()); }
    if short_version(&fresh).as_deref() != Some(release.version.as_str()) { return Err("the update's version doesn't match its release".into()); }
    step("Installing");
    let old = parent.join(format!(".Pointer-replaced-{}.app", std::process::id()));
    std::fs::rename(&current, &old).map_err(|e| format!("couldn't move the old app aside: {e}"))?;
    let moved = std::fs::rename(&fresh, &current).or_else(|_| {
        // Across volumes: copy instead.
        let out = run("/usr/bin/ditto", &[&fresh.to_string_lossy(), &current.to_string_lossy()])?;
        if out.status.success() { Ok(()) } else { Err("copy failed".to_string()) }
    });
    if let Err(e) = moved {
        let _ = std::fs::rename(&old, &current);
        return Err(format!("couldn't put the new app in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_dir_all(&work);
    Ok(current)
}

/// `Pointer --update`: checks for, installs and relaunches into an update from the terminal,
/// as Settings would. Returns the exit code.
pub fn run_cli() -> i32 {
    match check() {
        Ok(None) => {
            eprintln!("pointer: {} is the latest version", current());
            0
        }
        Ok(Some(release)) => match install(&release, |step| eprintln!("pointer: {step}…")) {
            Ok(app) => {
                eprintln!("pointer: installed {}, relaunching {}", release.version, app.display());
                relaunch(&app);
                0
            }
            Err(e) => {
                eprintln!("pointer: couldn't install {}: {e}", release.version);
                1
            }
        },
        Err(e) => {
            eprintln!("pointer: couldn't check for updates: {e}");
            1
        }
    }
}

/// Starts `app` once this process has gone (call, then quit).
pub fn relaunch(app: &Path) {
    let pid = std::process::id();
    let script = format!("while kill -0 {pid} 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"{}\"", app.display());
    let _ = Command::new("/bin/sh").arg("-c").arg(script).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("v1.0.1", "1.0.0"));
        assert!(newer("1.10.0", "1.9.9"));
        assert!(newer("2.0", "1.99.99"));
        assert!(!newer("v1.0.0", "1.0.0"));
        assert!(!newer("0.9.9", "1.0.0"));
        assert!(!newer("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn a_development_build_has_no_bundle() {
        assert!(bundle().is_none());
    }
}
