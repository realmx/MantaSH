//! Safe display metadata. Labels never become identifiers or command input.

pub fn clean(text: &str, limit: usize) -> String {
    text.chars().filter(|c|!c.is_control()&&!matches!(c,'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')).take(limit).collect::<String>().trim().to_owned()
}
/// A verified Shell command event may provide a conservative executable name, never arguments.
pub fn reported_program(command: &str) -> Option<String> {
    let token = command.split_whitespace().next()?;
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./\\:-".contains(c))
    {
        return None;
    }
    let name = token.rsplit(['/', '\\']).next()?;
    if name.is_empty() || !name.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    Some(name.chars().take(80).collect())
}
pub fn directory_name(directory: &str) -> String {
    let value = clean(directory, 4096);
    if value == "/" || value == "~" {
        return value;
    }
    if value.len() == 3
        && value.as_bytes()[1] == b':'
        && matches!(value.as_bytes()[2], b'/' | b'\\')
    {
        return value;
    }
    value
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_owned()
}
/// The active local pane determines the label; missing metadata falls back to the shell name.
pub fn local_label(directory: &str, program: Option<&str>, shell: &str) -> String {
    let directory = directory_name(directory);
    let program = program.map(|p| clean(p, 80)).filter(|p| !p.is_empty());
    match (program, directory.is_empty()) {
        (Some(program), false) => format!("{program} · {directory}"),
        (Some(program), true) => program,
        (None, false) => directory,
        (None, true) => directory_name(shell),
    }
}
