//! Exercise the real Bash/Zsh hooks with disposable HOME directories, without SSH.
#![cfg(unix)]

use mantash::{encoding::Encoding, integration, terminal::TerminalBuffer};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

const TOKEN: &str = "ssh-prompt-test";
const READY: &str = "\x1b]777;mantash-cursor;ssh-prompt-test;ready\x07";

#[derive(Clone, Copy, Debug)]
enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    fn path(self) -> &'static str {
        match self {
            Self::Bash => "/bin/bash",
            Self::Zsh => "/bin/zsh",
        }
    }

    fn marker(self) -> String {
        match self {
            Self::Bash => format!(r"\[\e]777;mantash-cursor;{TOKEN};ready\a\]"),
            Self::Zsh => format!("%{{{READY}%}}"),
        }
    }

    fn colored(self, prompt: &str) -> String {
        match self {
            Self::Bash => format!(r"\[\e[36m\]{prompt}\[\e[39m\]"),
            Self::Zsh => format!("%F{{cyan}}{prompt}%f"),
        }
    }
}

fn shells() -> impl Iterator<Item = Shell> {
    [Shell::Bash, Shell::Zsh].into_iter().filter(|shell| {
        if Path::new(shell.path()).is_file() {
            true
        } else {
            eprintln!("skipping {shell:?}: {} is not installed", shell.path());
            false
        }
    })
}

fn isolated_command(program: &str, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("HOME", home)
        .env("TMPDIR", home)
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("LC_ALL", "C")
        .env("HISTFILE", "/dev/null")
        .env("MANTASH_HISTORY_TOKEN", TOKEN)
        .current_dir(home);
    command
}

// Invoke the registered hooks, not a copy of their color logic. Disable Bash's
// DEBUG reporting after installation so only explicit NUL-delimited snapshots
// reach stdout; its cursor support decision remains intact.
fn snapshots(shell: Shell, ssh: bool, body: &str) -> Vec<String> {
    let home = tempfile::tempdir().unwrap();
    let hooks = home.path().join("hooks");
    integration::write_local(&hooks).unwrap();
    let (rc, invoke) = match shell {
        Shell::Bash => (
            "bash.rc",
            "trap - DEBUG\ndraw() { eval \"$PROMPT_COMMAND\" >/dev/null; }",
        ),
        Shell::Zsh => (
            ".zshrc",
            "draw() { local hook; for hook in \"${precmd_functions[@]}\"; do \"$hook\" >/dev/null; done; }",
        ),
    };
    let script = format!(
        "{}\nsource {}\n{invoke}\nsnapshot() {{ printf '%s\\0' \"$PS1\"; }}\n{body}",
        if ssh {
            "_mantash_ssh_prompt_color=1"
        } else {
            ""
        },
        integration::quote(hooks.join(rc).to_str().unwrap()),
    );
    let mut command = isolated_command(shell.path(), home.path());
    match shell {
        Shell::Bash => {
            command.args(["--noprofile", "--norc"]);
        }
        Shell::Zsh => {
            command.args(["-d", "-f"]);
        }
    }
    let output = command.args(["-c", &script]).output().unwrap();
    assert!(output.status.success(), "{shell:?}: {output:?}");
    assert!(output.stderr.is_empty(), "{shell:?}: {output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.ends_with('\0'),
        "missing snapshot for {shell:?}: {text:?}"
    );
    text.strip_suffix('\0')
        .unwrap()
        .split('\0')
        .map(str::to_owned)
        .collect()
}

#[test]
fn ssh_plain_prompts_reset_foreground_and_survive_repeated_and_dynamic_draws() {
    for shell in shells() {
        let actual = snapshots(
            shell,
            true,
            r"
PS1='first> '
draw; snapshot
draw; snapshot
PS1='replacement> '
draw; snapshot
draw; snapshot
",
        );
        let first = format!("{}{}", shell.colored("first> "), shell.marker());
        let replacement = format!("{}{}", shell.colored("replacement> "), shell.marker());
        assert_eq!(
            actual,
            [first.clone(), first, replacement.clone(), replacement],
            "{shell:?}"
        );
    }
}

#[test]
fn ssh_explicit_colors_are_preserved_even_after_a_plain_prompt() {
    for shell in shells() {
        let prompts: Vec<&str> = match shell {
            Shell::Bash => vec![
                r"\[\e[31m\]red> \[\e[0m\]",
                r"\[\E[32m\]green> \[\E[0m\]",
                r"\[\033[34m\]blue> \[\033[0m\]",
                r"\[\x1b[35m\]purple> \[\x1b[0m\]",
                r"\[\x1B[38;5;123m\]indexed> \[\x1B[0m\]",
                "\x1b[38;2;12;34;56mrgb> \x1b[0m",
            ],
            Shell::Zsh => vec![
                "%F{red}red> %f",
                "%K{blue}background> %k",
                "default> %f",
                "default> %k",
                "%{\x1b[38;2;12;34;56m%}rgb> %{\x1b[0m%}",
            ],
        };
        let mut body = String::from("PS1='plain> '\ndraw\n");
        for prompt in &prompts {
            body.push_str(&format!(
                "PS1={}\ndraw; snapshot\ndraw; snapshot\n",
                integration::quote(prompt)
            ));
        }
        let expected: Vec<_> = prompts
            .iter()
            .flat_map(|prompt| {
                let value = format!("{prompt}{}", shell.marker());
                [value.clone(), value]
            })
            .collect();
        assert_eq!(snapshots(shell, true, &body), expected, "{shell:?}");
    }
}

#[test]
fn local_plain_and_replaced_prompts_receive_only_the_cursor_marker() {
    for shell in shells() {
        let actual = snapshots(
            shell,
            false,
            r"
PS1='local> '
draw; snapshot
draw; snapshot
PS1='changed> '
draw; snapshot
",
        );
        assert_eq!(
            actual,
            [
                format!("local> {}", shell.marker()),
                format!("local> {}", shell.marker()),
                format!("changed> {}", shell.marker()),
            ],
            "{shell:?}"
        );
    }
}

#[test]
fn ssh_empty_prompt_does_not_gain_color_on_its_first_draw() {
    for shell in shells() {
        assert_eq!(
            snapshots(shell, true, "PS1=''\ndraw; snapshot"),
            [shell.marker()],
            "{shell:?}"
        );
    }
}

#[test]
fn remote_bootstrap_colors_real_prompts_without_exporting_the_flag() {
    for shell in shells() {
        let home = tempfile::tempdir().unwrap();
        // The only user rc files either shell can see belong to this fixture.
        for rc in [".bashrc", ".zshrc"] {
            std::fs::write(
                home.path().join(rc),
                "PS1='remote> '\nHISTFILE=\"$HOME/history\"\n",
            )
            .unwrap();
        }
        let mut child = isolated_command("/bin/sh", home.path())
            .env("SHELL", shell.path())
            .args(["-c", &integration::remote_command(TOKEN)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                br#"printf 'FLAG=%s CHILD=%s\n' "$_mantash_ssh_prompt_color" "$(/bin/sh -c 'printf %s "${_mantash_ssh_prompt_color-unset}"')"
exit
"#,
            )
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{shell:?}: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("FLAG=1 CHILD=unset"), "{shell:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let rendered = format!("\x1b[36mremote> \x1b[39m{READY}");
        assert!(stderr.contains(&rendered), "{shell:?}: {output:?}");

        // Feed the actual shell-rendered bytes through the application parser:
        // color and ready escapes must not add columns to the input boundary.
        let start = stderr.find(&rendered).unwrap();
        let mut terminal = TerminalBuffer::new(Encoding::Utf8);
        terminal.resize(80, 6);
        terminal.set_shell_token(TOKEN.into());
        terminal.feed(&stderr.as_bytes()[start..start + rendered.len()]);
        assert!(terminal.command_cursor.editing(), "{shell:?}");
        terminal.feed(b"abc");
        assert_eq!(
            terminal.cursor_movement(8, 0),
            Some(b"\x1b[D\x1b[D".to_vec()),
            "{shell:?}"
        );
        assert!(terminal.cursor_movement(0, 0).is_none(), "{shell:?}");
    }
}
