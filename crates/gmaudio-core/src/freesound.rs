//! Freesound (freesound.org): search its sound library and download sounds
//! into the user's own.
//!
//! Two levels of access, as Freesound's API defines them:
//!
//! - An **API key** is enough to search and to download previews
//!   (compressed copies, good enough for most table use).
//! - Signing in with a Freesound account (**OAuth2**) unlocks the original
//!   files as their authors uploaded them.
//!
//! Sign-in works without a web server: the browser shows a code, the GM
//! pastes it into the app, and the app exchanges it for tokens.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const API: &str = "https://freesound.org/apiv2";
/// Formats the app can play. Anything else is fetched as its preview.
const PLAYABLE: [&str; 4] = ["wav", "flac", "ogg", "mp3"];
const PAGE_SIZE: u32 = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    /// Freesound calls this the "client secret / API key".
    pub api_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds since 1970 at which the access token stops working.
    pub expires_at: i64,
}

impl Tokens {
    pub fn is_fresh(&self) -> bool {
        self.expires_at > now() + 120
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sound {
    pub id: u64,
    pub name: String,
    pub username: String,
    /// Short licence name: "CC0", "CC BY", "CC BY-NC"...
    pub license: String,
    pub license_url: String,
    pub duration: f64,
    pub tags: Vec<String>,
    /// The original file's format.
    pub kind: String,
    pub filesize: u64,
    /// A compressed copy for listening before downloading.
    pub preview_url: String,
}

impl Sound {
    pub fn page_url(&self) -> String {
        format!("https://freesound.org/s/{}/", self.id)
    }

    /// The credit line this sound's licence asks for.
    pub fn attribution(&self) -> String {
        format!("\"{}\" by {} ({}), {}", self.name, self.username, self.page_url(), self.license)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchPage {
    /// How many sounds match in total.
    pub count: u64,
    pub sounds: Vec<Sound>,
    pub has_more: bool,
}

// ---- wire formats ------------------------------------------------------------

#[derive(Deserialize)]
struct RawPage {
    count: u64,
    next: Option<String>,
    results: Vec<RawSound>,
}

#[derive(Deserialize)]
struct RawSound {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    filesize: u64,
    #[serde(default)]
    previews: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct RawTokens {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_in: i64,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "https://creativecommons.org/licenses/by-nc/4.0/" -> "CC BY-NC".
pub fn license_name(url: &str) -> String {
    let url = url.to_lowercase();
    if url.contains("publicdomain/zero") {
        "CC0".into()
    } else if url.contains("/by-nc") {
        "CC BY-NC".into()
    } else if url.contains("/by/") {
        "CC BY".into()
    } else if url.contains("sampling") {
        "Sampling+".into()
    } else {
        "See Freesound".into()
    }
}

fn parse_page(json: &str) -> Result<SearchPage> {
    let raw: RawPage = serde_json::from_str(json).map_err(|e| Error::Online(format!("unexpected answer from Freesound: {e}")))?;
    Ok(SearchPage {
        count: raw.count,
        has_more: raw.next.is_some(),
        sounds: raw
            .results
            .into_iter()
            .filter_map(|s| {
                // Prefer the Ogg preview: smaller, and no encoder delay at the start.
                let preview_url = s.previews.get("preview-hq-ogg").or_else(|| s.previews.get("preview-hq-mp3"))?.clone();
                Some(Sound {
                    id: s.id,
                    name: s.name,
                    username: s.username,
                    license: license_name(&s.license),
                    license_url: s.license,
                    duration: s.duration,
                    tags: s.tags,
                    kind: s.kind.to_lowercase(),
                    filesize: s.filesize,
                    preview_url,
                })
            })
            .collect(),
    })
}

fn parse_tokens(json: &str) -> Result<Tokens> {
    let raw: RawTokens =
        serde_json::from_str(json).map_err(|_| Error::Online("Freesound did not accept that. Check the code and try again.".into()))?;
    Ok(Tokens {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_at: now() + if raw.expires_in > 0 { raw.expires_in } else { 86_400 },
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .user_agent("GMAudio")
        .build()
}

/// Turns a failed request into words the GM can act on.
fn explain(e: ureq::Error) -> Error {
    Error::Online(match e {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
            "Freesound refused the key or sign-in. Check them under Freesound settings.".into()
        }
        ureq::Error::Status(404, _) => "Freesound no longer has that sound.".into(),
        ureq::Error::Status(429, _) => "Freesound's daily limit for this key has been reached. Try again later.".into(),
        ureq::Error::Status(code, _) => format!("Freesound answered with an error ({code})."),
        ureq::Error::Transport(_) => "Could not reach Freesound. Check the internet connection.".into(),
    })
}

// ---- the API -----------------------------------------------------------------

/// Searches by text. Pages are numbered from 1.
pub fn search(api_key: &str, query: &str, page: u32) -> Result<SearchPage> {
    let body = agent()
        .get(&format!("{API}/search/text/"))
        .query("query", query)
        .query("page", &page.max(1).to_string())
        .query("page_size", &PAGE_SIZE.to_string())
        .query("fields", "id,name,username,license,duration,tags,type,filesize,previews")
        .query("token", api_key)
        .call()
        .map_err(explain)?
        .into_string()
        .map_err(|e| Error::Online(e.to_string()))?;
    parse_page(&body)
}

/// The page to open in a browser to sign in. It ends by showing a code.
pub fn authorize_url(client_id: &str) -> String {
    format!("{API}/oauth2/authorize/?client_id={client_id}&response_type=code")
}

fn token_request(credentials: &Credentials, grant: &[(&str, &str)]) -> Result<Tokens> {
    let mut form = vec![("client_id", credentials.client_id.as_str()), ("client_secret", credentials.api_key.as_str())];
    form.extend_from_slice(grant);
    let body = agent()
        .post(&format!("{API}/oauth2/access_token/"))
        .send_form(&form)
        .map_err(|e| match e {
            ureq::Error::Status(400, _) | ureq::Error::Status(401, _) => {
                Error::Online("Freesound did not accept that. Check the code and try again.".into())
            }
            other => explain(other),
        })?
        .into_string()
        .map_err(|e| Error::Online(e.to_string()))?;
    parse_tokens(&body)
}

/// Trades the code shown after signing in for tokens.
pub fn exchange_code(credentials: &Credentials, code: &str) -> Result<Tokens> {
    token_request(credentials, &[("grant_type", "authorization_code"), ("code", code.trim())])
}

/// Gets a new access token when the old one has run out (they last a day).
pub fn refresh(credentials: &Credentials, tokens: &Tokens) -> Result<Tokens> {
    token_request(credentials, &[("grant_type", "refresh_token"), ("refresh_token", &tokens.refresh_token)])
}

/// A file name that is safe on Windows and tells files with equal names apart.
pub fn file_name(sound: &Sound, extension: &str) -> String {
    let stem = Path::new(&sound.name).file_stem().and_then(|s| s.to_str()).unwrap_or(&sound.name);
    let clean: String = stem
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_matches(|c: char| c == '.' || c == ' ');
    let clean: String = clean.chars().take(80).collect();
    let stem = if clean.is_empty() { "sound" } else { clean.as_str() };
    format!("{stem} [fs{}].{extension}", sound.id)
}

fn save(response: ureq::Response, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Download beside the target and rename, so a broken download never
    // leaves half a file in the library.
    let partial = target.with_extension("part");
    let mut file = std::fs::File::create(&partial)?;
    let copied = std::io::copy(&mut response.into_reader().take(2_000_000_000), &mut file);
    drop(file);
    match copied {
        Ok(bytes) if bytes > 0 => Ok(std::fs::rename(&partial, target)?),
        Ok(_) => {
            let _ = std::fs::remove_file(&partial);
            Err(Error::Online("Freesound sent an empty file.".into()))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(Error::Online(format!("The download was cut short: {e}")))
        }
    }
}

/// Downloads a sound into `folder` and returns where it was saved.
///
/// With an access token the original file is fetched, if it is in a format
/// the app can play; otherwise the preview.
pub fn download(sound: &Sound, access_token: Option<&str>, folder: &Path) -> Result<PathBuf> {
    let original = access_token.filter(|_| PLAYABLE.contains(&sound.kind.as_str()));
    let (response, extension) = match original {
        Some(token) => (
            agent()
                .get(&format!("{API}/sounds/{}/download/", sound.id))
                .set("Authorization", &format!("Bearer {token}"))
                .call()
                .map_err(explain)?,
            sound.kind.as_str(),
        ),
        None => {
            let extension = if sound.preview_url.ends_with(".mp3") { "mp3" } else { "ogg" };
            (agent().get(&sound.preview_url).call().map_err(explain)?, extension)
        }
    };
    let target = folder.join(file_name(sound, extension));
    save(response, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"{
        "count": 2412, "next": "https://freesound.org/apiv2/search/text/?query=door&page=2", "previous": null,
        "results": [
            {"id": 15419, "name": "Door: creak?.wav", "username": "inferno", "license": "https://creativecommons.org/licenses/by/4.0/",
             "duration": 2.57, "tags": ["door", "creak"], "type": "wav", "filesize": 452312,
             "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/15/15419_hq.mp3",
                          "preview-hq-ogg": "https://cdn.freesound.org/previews/15/15419_hq.ogg"}},
            {"id": 99, "name": "slam", "username": "x", "license": "http://creativecommons.org/publicdomain/zero/1.0/",
             "duration": 1.0, "tags": [], "type": "AIFF", "filesize": 10,
             "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/0/99_hq.mp3"}},
            {"id": 100, "name": "no previews at all", "previews": {}}
        ]}"#;

    #[test]
    fn reads_a_search_page() {
        let page = parse_page(PAGE).unwrap();
        assert_eq!(page.count, 2412);
        assert!(page.has_more);
        assert_eq!(page.sounds.len(), 2, "a sound with nothing to listen to is left out");
        let door = &page.sounds[0];
        assert_eq!((door.id, door.license.as_str(), door.kind.as_str()), (15419, "CC BY", "wav"));
        assert!(door.preview_url.ends_with("_hq.ogg"), "Ogg preview preferred");
        assert_eq!(door.attribution(), "\"Door: creak?.wav\" by inferno (https://freesound.org/s/15419/), CC BY");
        assert_eq!((page.sounds[1].license.as_str(), page.sounds[1].kind.as_str()), ("CC0", "aiff"));
        assert!(page.sounds[1].preview_url.ends_with(".mp3"));

        assert!(matches!(parse_page("<html>down for maintenance</html>"), Err(Error::Online(_))));
    }

    #[test]
    fn file_names_are_safe_and_distinct() {
        let page = parse_page(PAGE).unwrap();
        assert_eq!(file_name(&page.sounds[0], "wav"), "Door creak [fs15419].wav");
        assert_eq!(file_name(&page.sounds[1], "mp3"), "slam [fs99].mp3");
        let odd = Sound { name: "  ...  ".into(), ..page.sounds[1].clone() };
        assert_eq!(file_name(&odd, "ogg"), "sound [fs99].ogg");
    }

    #[test]
    fn reads_tokens_and_knows_when_they_expire() {
        let tokens = parse_tokens(r#"{"access_token":"a","refresh_token":"r","expires_in":86400,"scope":"read write"}"#).unwrap();
        assert_eq!((tokens.access_token.as_str(), tokens.refresh_token.as_str()), ("a", "r"));
        assert!(tokens.is_fresh());
        assert!(!Tokens { expires_at: now() + 30, ..tokens }.is_fresh());
        // An error answer (a wrong code) is reported in plain words.
        let failed = parse_tokens(r#"{"error":"invalid_grant"}"#).unwrap_err();
        assert!(failed.to_string().contains("did not accept"));
    }

    #[test]
    fn builds_the_sign_in_address() {
        assert_eq!(
            authorize_url("abc123"),
            "https://freesound.org/apiv2/oauth2/authorize/?client_id=abc123&response_type=code"
        );
        assert_eq!(license_name("https://creativecommons.org/licenses/by-nc/3.0/"), "CC BY-NC");
        assert_eq!(license_name("https://creativecommons.org/licenses/sampling+/1.0/"), "Sampling+");
    }
}
