//! Audio from a YouTube link, saved into the library.
//!
//! The work is done by `yt-dlp`, a separate program that GMAudio fetches
//! from its GitHub releases the first time and keeps up to date (YouTube
//! changes often, and an old yt-dlp stops working). The audio is taken as
//! M4A, which YouTube already offers, so nothing has to be converted and
//! no other program is needed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::{Error, Result};

#[cfg(windows)]
const PROGRAM: &str = "yt-dlp.exe";
#[cfg(target_os = "macos")]
const PROGRAM: &str = "yt-dlp_macos";
#[cfg(all(not(windows), not(target_os = "macos")))]
const PROGRAM: &str = "yt-dlp_linux";

const RELEASES: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
/// How long before yt-dlp is asked to update itself again.
const UPDATE_EVERY: Duration = Duration::from_secs(3 * 24 * 60 * 60);

fn online(text: impl Into<String>) -> Error {
    Error::Online(text.into())
}

/// Only links to a single YouTube video are accepted.
pub fn check_url(url: &str) -> Result<String> {
    let url = url.trim();
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    let host = rest.split(['/', '?']).next().unwrap_or("").to_ascii_lowercase();
    let host = host.strip_prefix("www.").or_else(|| host.strip_prefix("m.")).or_else(|| host.strip_prefix("music.")).unwrap_or(&host);
    if !matches!(host, "youtube.com" | "youtu.be") || rest.len() <= host.len() + 1 || url.contains(char::is_whitespace) {
        return Err(online("That does not look like a YouTube link. Copy the address of the video from your browser."));
    }
    Ok(format!("https://{rest}"))
}

fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Makes sure yt-dlp is in `tools` and reasonably fresh. Returns its path.
pub fn ensure_program(tools: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(tools)?;
    let program = tools.join(PROGRAM);
    if !program.exists() {
        let response = ureq::get(&format!("{RELEASES}/{PROGRAM}"))
            .timeout(Duration::from_secs(300))
            .call()
            .map_err(|e| online(format!("The YouTube helper (yt-dlp) could not be downloaded: {e}")))?;
        let partial = tools.join(format!("{PROGRAM}.part"));
        std::io::copy(&mut response.into_reader(), &mut std::fs::File::create(&partial)?)?;
        std::fs::rename(&partial, &program)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        }
        return Ok(program);
    }
    let stamp = tools.join("yt-dlp.checked");
    let stale = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .map_or(true, |at| SystemTime::now().duration_since(at).unwrap_or_default() > UPDATE_EVERY);
    if stale {
        update(&program);
        let _ = std::fs::write(&stamp, "");
    }
    Ok(program)
}

/// Asks yt-dlp to update itself. Failing is fine: the old one is tried.
fn update(program: &Path) {
    let _ = command(program).arg("-U").stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// What yt-dlp is asked to do for one link.
fn arguments(url: &str, folder: &Path) -> Vec<String> {
    [
        "--no-playlist",
        "--no-warnings",
        "--no-progress",
        "--windows-filenames",
        "--no-overwrites",
        // M4A plays as it is; anything else would need converting.
        "-f",
        "bestaudio[ext=m4a]/best[ext=mp4]",
        "--remux-video",
        "m4a",
        "--print",
        "after_move:filepath",
        "-o",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([folder.join("%(title).150B [%(id)s].%(ext)s").to_string_lossy().to_string(), url.to_string()])
    .collect()
}

/// The reason yt-dlp gave, in a form worth showing.
fn explain(stderr: &str) -> String {
    let reason = stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("ERROR:"))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| stderr.lines().last().unwrap_or("no reason given").trim().to_string());
    format!("YouTube did not hand over the audio: {reason}")
}

/// Saves the audio of one video into `folder`. Returns the file's path.
pub fn download(tools: &Path, url: &str, folder: &Path) -> Result<PathBuf> {
    let url = check_url(url)?;
    let program = ensure_program(tools)?;
    std::fs::create_dir_all(folder)?;
    let run = || command(&program).args(arguments(&url, folder)).output();
    let mut output = run()?;
    if !output.status.success() {
        // Most failures are an out-of-date yt-dlp: update and try once more.
        update(&program);
        let _ = std::fs::write(tools.join("yt-dlp.checked"), "");
        output = run()?;
    }
    if !output.status.success() {
        return Err(online(explain(&String::from_utf8_lossy(&output.stderr))));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let path = printed.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(PathBuf::from);
    match path {
        Some(path) if path.exists() => {
            let playable = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("m4a"));
            if playable {
                Ok(path)
            } else {
                Err(online("That video has no audio in a form GMAudio can play (M4A)."))
            }
        }
        _ => Err(online("The download finished but the file could not be found.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_youtube_video_links_are_accepted() {
        for good in [
            "https://www.youtube.com/watch?v=abc123",
            "youtube.com/watch?v=abc123",
            " https://youtu.be/abc123 ",
            "https://music.youtube.com/watch?v=abc123",
            "http://m.youtube.com/watch?v=abc123",
        ] {
            assert!(check_url(good).is_ok(), "{good}");
        }
        assert_eq!(check_url("youtu.be/abc").unwrap(), "https://youtu.be/abc");
        for bad in ["", "https://example.com/watch?v=1", "https://youtube.com", "https://youtube.com.evil.io/x", "--exec rm", "https://youtu.be/a b"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_reason_for_a_failure_is_the_error_line() {
        let stderr = "WARNING: something\nERROR: [youtube] abc: Video unavailable\n";
        assert_eq!(explain(stderr), "YouTube did not hand over the audio: [youtube] abc: Video unavailable");
        assert!(explain("").contains("no reason given"));
    }

    #[test]
    fn the_file_is_saved_as_title_and_id_in_the_folder() {
        let args = arguments("https://youtu.be/abc", Path::new("/lib/YouTube"));
        assert_eq!(args.last().unwrap(), "https://youtu.be/abc");
        assert!(args[args.len() - 2].starts_with("/lib/YouTube"));
        assert!(args.contains(&"--no-playlist".to_string()));
    }
}
