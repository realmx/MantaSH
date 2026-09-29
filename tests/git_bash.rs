//! Git for Windows prompt fixtures executed by Bash without loading user configuration.
use mantash::integration;

#[test]
fn local_bash_prompt_adjustment_is_windows_only() {
    let directory = tempfile::tempdir().unwrap();
    integration::write_local(directory.path()).unwrap();
    let rc = std::fs::read_to_string(directory.path().join("bash.rc")).unwrap();
    let expected = if cfg!(windows) {
        format!("{}{}", integration::BASH_RC, integration::GIT_BASH_PROMPT)
    } else {
        integration::BASH_RC.to_string()
    };
    assert_eq!(rc, expected);
    assert!(!integration::remote_command("test-token").contains("_mantash_git_prefix"));
}

#[cfg(unix)]
mod bash {
    use super::*;
    use std::process::Command;

    // Prefix and two-line structure from Git for Windows' git-extra/git-prompt.sh.
    const PROMPT: &str = r"\[\033]0;$TITLEPREFIX:$PWD\007\]\n\[\033[32m\]\u@\h \[\033[35m\]$MSYSTEM \[\033[33m\]\w\[\033[36m\]`__git_ps1`\[\033[0m\]\n$ ";

    fn adjusted(prompt: &str, ostype: &str, custom_file: bool) -> String {
        let home = tempfile::tempdir().unwrap();
        if custom_file {
            std::fs::create_dir_all(home.path().join(".config/git")).unwrap();
            std::fs::write(
                home.path().join(".config/git/git-prompt.sh"),
                "# user prompt\n",
            )
            .unwrap();
        }
        let script = format!(
            "OSTYPE={}\nPS1={}\n{}\n{}\nprintf '%s' \"$PS1\"",
            integration::quote(ostype),
            integration::quote(prompt),
            integration::GIT_BASH_PROMPT,
            integration::GIT_BASH_PROMPT,
        );
        let output = Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-c", &script])
            .env_remove("BASH_ENV")
            .env("HOME", home.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stderr.is_empty(), "{:?}", output);
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn git_bash_removes_only_the_default_leading_newline_and_is_idempotent() {
        assert_eq!(
            adjusted(PROMPT, "msys", false),
            PROMPT.replacen(r"\n", "", 1)
        );
    }

    #[test]
    fn custom_prompts_and_other_bash_platforms_are_unchanged() {
        assert_eq!(adjusted(PROMPT, "linux-gnu", false), PROMPT);
        assert_eq!(adjusted(PROMPT, "msys", true), PROMPT);
        let custom = r"\ncustom \w\n$ ";
        assert_eq!(adjusted(custom, "msys", false), custom);
    }
}
