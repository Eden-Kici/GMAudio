//! Keys and sign-in tokens, kept in the operating system's credential
//! store (Windows Credential Manager) rather than in the library file.
//!
//! If that store cannot be reached, the value is kept in the library's
//! settings instead, so the feature still works.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::library::Library;

const SERVICE: &str = "GMAudio";

/// Values set or read while the app has been running. Some credential
/// stores accept a value and then cannot return it to another thread;
/// this keeps the app working until it is closed regardless.
static REMEMBERED: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn remembered(name: &str) -> Option<Option<String>> {
    REMEMBERED.lock().unwrap().as_ref()?.get(name).cloned()
}

fn remember(name: &str, value: Option<&str>) {
    REMEMBERED
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(name.to_string(), value.map(str::to_string));
}

fn fallback_key(name: &str) -> String {
    format!("secret.{name}")
}

pub fn get(library: &Library, name: &str) -> Option<String> {
    if let Some(known) = remembered(name) {
        return known;
    }
    let stored = keyring::Entry::new(SERVICE, name).and_then(|entry| entry.get_password()).ok();
    let value = stored.or_else(|| library.get_setting(&fallback_key(name)).ok().flatten().filter(|v| !v.is_empty()));
    remember(name, value.as_deref());
    value
}

pub fn set(library: &Library, name: &str, value: &str) {
    let stored = keyring::Entry::new(SERVICE, name).and_then(|entry| entry.set_password(value)).is_ok();
    // Keep exactly one copy: in the settings only when the store refused it.
    let _ = library.set_setting(&fallback_key(name), if stored { "" } else { value });
    remember(name, Some(value));
}

pub fn delete(library: &Library, name: &str) {
    if let Ok(entry) = keyring::Entry::new(SERVICE, name) {
        let _ = entry.delete_credential();
    }
    let _ = library.set_setting(&fallback_key(name), "");
    remember(name, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_and_can_be_deleted() {
        let library = Library::in_memory(|_| {}).unwrap();
        let name = format!("test.{}", std::process::id());
        assert_eq!(get(&library, &name), None);
        set(&library, &name, "s3cret");
        assert_eq!(get(&library, &name).as_deref(), Some("s3cret"));
        set(&library, &name, "changed");
        assert_eq!(get(&library, &name).as_deref(), Some("changed"));
        delete(&library, &name);
        assert_eq!(get(&library, &name), None);
    }
}
