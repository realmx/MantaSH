//! Versioned connection import/export with preview before mutation.
use crate::model::{Id, Profile};
use anyhow::{Result, bail, ensure};
use serde::Deserialize;

/// Search public connection metadata by whitespace-separated terms, including endpoint and port.
pub fn matches_query(profile: &Profile, query: &str) -> bool {
    let text = format!("{} {}", profile.name, profile.endpoint()).to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}

/// Clone public metadata into an unsaved identity; the new UUID cannot read the source credential.
pub fn clone_profile(profile: &Profile, name: String) -> Profile {
    Profile {
        id: Id::new_v4(),
        name,
        ..profile.clone()
    }
}

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub row: usize,
    pub profile: Option<Profile>,
    /// Password supplied by the import file; kept beside the row, never inside Profile.
    pub password: Option<String>,
    pub duplicate: Option<Id>,
    pub error: Option<String>,
}
#[derive(Debug, Clone)]
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
}

/// Parse CSV records independently so a malformed row cannot erase valid data.
/// A `password` field is captured per row for explicit vault import and never
/// becomes part of Profile.
pub fn preview(text: &str, existing: &[Profile]) -> Result<ImportPreview> {
    if text.len() > 8 * 1024 * 1024 {
        bail!("Connection import exceeds the 8 MiB limit");
    }
    #[derive(Deserialize)]
    struct CsvRow {
        name: String,
        host: String,
        #[serde(default = "default_port")]
        port: u16,
        username: String,
        #[serde(default)]
        password: String,
    }
    fn default_port() -> u16 {
        22
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    // One format only: the header must contain exactly five unique native columns.
    let header = reader.headers()?.clone();
    let expected = ["name", "host", "port", "username", "password"];
    ensure!(
        header.len() == expected.len(),
        "Connection CSV must contain exactly the five name, host, port, username and password columns"
    );
    let mut present = [false; 5];
    let mut normalized_header = csv::StringRecord::new();
    for column in &header {
        let Some(index) = expected
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(column))
        else {
            bail!("Unsupported connection CSV column '{column}'");
        };
        ensure!(
            !present[index],
            "Duplicate connection CSV column '{column}'"
        );
        present[index] = true;
        normalized_header.push_field(expected[index]);
    }
    ensure!(
        present.iter().all(|seen| *seen),
        "Connection CSV must contain the name, host, port, username and password columns"
    );
    reader.set_headers(normalized_header);
    let parsed = reader
        .deserialize::<CsvRow>()
        .map(|r| {
            let password = r
                .as_ref()
                .ok()
                .and_then(|row| (!row.password.is_empty()).then(|| row.password.clone()));
            let profile = r.map(|row| Profile {
                id: Id::new_v4(),
                name: row.name,
                host: row.host,
                port: row.port,
                username: row.username,
            });
            (profile.map_err(|e| e.to_string()), password)
        })
        .collect::<Vec<_>>();
    let mut seen = existing.to_vec();
    let mut rows = Vec::new();
    for (index, (parsed, password)) in parsed.into_iter().enumerate() {
        match parsed {
            Err(error) => rows.push(ImportRow {
                row: index + 1,
                profile: None,
                password: None,
                duplicate: None,
                error: Some(error),
            }),
            Ok(profile) => match profile.validate() {
                Err(error) => rows.push(ImportRow {
                    row: index + 1,
                    profile: None,
                    password: None,
                    duplicate: None,
                    error: Some(error),
                }),
                Ok(()) => {
                    let duplicate = seen.iter().find(|p| p.duplicates(&profile)).map(|p| p.id);
                    if duplicate.is_none() {
                        seen.push(profile.clone());
                    }
                    rows.push(ImportRow {
                        row: index + 1,
                        profile: Some(profile),
                        password,
                        duplicate,
                        error: None,
                    });
                }
            },
        }
    }
    Ok(ImportPreview { rows })
}

/// Merge only validated rows; matching records retain their history/credential identity.
pub fn merge(existing: &[Profile], preview: &ImportPreview, replace: bool) -> Vec<Profile> {
    let mut output = existing.to_vec();
    for row in &preview.rows {
        if let Some(profile) = &row.profile {
            if let Some(index) = output.iter().position(|p| p.duplicates(profile)) {
                if replace {
                    let mut p = profile.clone();
                    p.id = output[index].id;
                    output[index] = p;
                }
            } else {
                output.push(profile.clone());
            }
        }
    }
    output
}

/// Export as CSV with the stored password (when available) after the username.
/// The id column is omitted; duplicates on re-import match by endpoint identity.
pub fn export(
    profiles: &[Profile],
    secrets: &std::collections::HashMap<Id, String>,
) -> Result<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(["name", "host", "port", "username", "password"])?;
    for p in profiles {
        writer.write_record([
            p.name.as_str(),
            p.host.as_str(),
            &p.port.to_string(),
            p.username.as_str(),
            secrets.get(&p.id).map(String::as_str).unwrap_or(""),
        ])?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?)
}
