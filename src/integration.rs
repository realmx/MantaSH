//! Session-local Shell hooks. User rc files are sourced, never edited.
use anyhow::Result;
use std::path::Path;

pub const ZSH_RC: &str = r#"ZDOTDIR=${MANTASH_USER_ZDOTDIR:-$HOME}
# /etc/zshrc runs before this file and derives HISTFILE from the overridden
# ZDOTDIR, sending history into the integration directory. Restore the user's
# real history file here, before their rc files run, so explicit user
# HISTFILE settings still win.
if [[ -z "$HISTFILE" || "$HISTFILE" == "$MANTASH_INTEGRATION_DIR"/.zsh_history ]]; then
  HISTFILE="${ZDOTDIR:-$HOME}/.zsh_history"
fi
[[ -r "$ZDOTDIR/.zshrc" ]] && source "$ZDOTDIR/.zshrc"
# Match terminal grapheme display when moving across combining characters.
setopt COMBINING_CHARS
function _mantash_preexec() {
  printf '\033]777;mantash-cursor;%s;running\007' "$MANTASH_HISTORY_TOKEN"
  [[ "$1" == ' '* ]] && return
  printf '\033]777;mantash;%s;%s;%s\007' "$MANTASH_HISTORY_TOKEN" "$(printf '%s' "$1" | base64 | tr -d '\r\n')" "$(printf '%s' "$PWD" | base64 | tr -d '\r\n')"
}
autoload -Uz add-zsh-hook
add-zsh-hook preexec _mantash_preexec
_mantash_prompt_end="%{"$'\e]777;mantash-cursor;'"$MANTASH_HISTORY_TOKEN"$';ready\a'"%}"
function _mantash_precmd() {
  [[ "$PROMPT" == *"$_mantash_prompt_end" ]] || PROMPT="${PROMPT}${_mantash_prompt_end}"
  printf '\033]777;mantash;%s;;%s\007' "$MANTASH_HISTORY_TOKEN" "$(printf '%s' "$PWD" | base64 | tr -d '\r\n')"
}
add-zsh-hook precmd _mantash_precmd
# Session-local defaults; explicit aliases/functions and command-line overrides win.
if [[ "$(whence -w vim 2>/dev/null)" == 'vim: command' ]]; then function vim { command vim -c 'set mouse=a' "$@"; }; fi
if [[ "$(whence -w nvim 2>/dev/null)" == 'nvim: command' ]]; then function nvim { command nvim -c 'set mouse=a' "$@"; }; fi
if [[ "$(whence -w vi 2>/dev/null)" == 'vi: command' ]] && command vi --version 2>/dev/null | command grep -q 'VIM'; then function vi { command vi -c 'set mouse=a' "$@"; }; fi
if [[ "$(whence -w nano 2>/dev/null)" == 'nano: command' ]]; then function nano {
  if [[ "$(whence -p nano)" == /usr/bin/nano && "$(command uname -s)" == Darwin ]]; then
    # macOS nano is Pico: its own xterm mouse decoder is gated on DISPLAY.
    printf '\033[?1006l\033[?1005l'
    DISPLAY="${DISPLAY:-:0}" command nano -m "$@"
  else
    command nano -m "$@"
  fi
}; fi
"#;
pub const ZSH_ENV: &str = r#"[[ -r "${MANTASH_USER_ZDOTDIR:-$HOME}/.zshenv" ]] && source "${MANTASH_USER_ZDOTDIR:-$HOME}/.zshenv"
ZDOTDIR=$MANTASH_INTEGRATION_DIR
"#;
pub const BASH_RC: &str = r#"[[ -r "$HOME/.bashrc" ]] && source "$HOME/.bashrc"
_mantash_ready=0
_mantash_history_line=$(HISTTIMEFORMAT= builtin history 1)
_mantash_history_line="${_mantash_history_line#"${_mantash_history_line%%[![:space:]]*}"}"
_mantash_last_history="${_mantash_history_line%%[[:space:]]*}"
_mantash_prompt_end='\[\e]777;mantash-cursor;'"$MANTASH_HISTORY_TOKEN"';ready\a\]'
_mantash_cursor_supported=0
_mantash_prompt() {
  if [[ $_mantash_cursor_supported == 1 && "$PS1" != *"$_mantash_prompt_end" ]]; then PS1="${PS1}${_mantash_prompt_end}"; fi
  printf '\033]777;mantash;%s;;%s\007' "$MANTASH_HISTORY_TOKEN" "$(printf '%s' "$PWD" | base64 | tr -d '\r\n')"
  _mantash_ready=1
}
_mantash_preexec() {
  [[ $_mantash_ready == 1 && "$BASH_COMMAND" != _mantash_prompt* && "$BASH_COMMAND" != *PROMPT_COMMAND* ]] || return
  _mantash_ready=0
  printf '\033]777;mantash-cursor;%s;running\007' "$MANTASH_HISTORY_TOKEN"
  local _mantash_line _mantash_number
  _mantash_line=$(HISTTIMEFORMAT= builtin history 1)
  _mantash_line="${_mantash_line#"${_mantash_line%%[![:space:]]*}"}"
  _mantash_number="${_mantash_line%%[[:space:]]*}"
  [[ -z "$_mantash_number" || "$_mantash_number" == "$_mantash_last_history" ]] && return
  _mantash_last_history=$_mantash_number
  _mantash_line="${_mantash_line#"$_mantash_number"  }"
  [[ -z "$_mantash_line" || "$_mantash_line" == ' '* ]] && return
  printf '\033]777;mantash;%s;%s;%s\007' "$MANTASH_HISTORY_TOKEN" "$(printf '%s' "$_mantash_line" | base64 | tr -d '\r\n')" "$(printf '%s' "$PWD" | base64 | tr -d '\r\n')"
}
if declare -p PROMPT_COMMAND 2>/dev/null | grep -q 'declare -a'; then
  PROMPT_COMMAND+=(_mantash_prompt)
else
  PROMPT_COMMAND="${PROMPT_COMMAND:+$PROMPT_COMMAND; }_mantash_prompt"
fi
if [[ -z "$(trap -p DEBUG)" ]]; then
  trap '_mantash_preexec' DEBUG
  _mantash_cursor_supported=1
fi
if [[ "$(type -t vim)" == file ]]; then function vim { command vim -c 'set mouse=a' "$@"; }; fi
if [[ "$(type -t nvim)" == file ]]; then function nvim { command nvim -c 'set mouse=a' "$@"; }; fi
if [[ "$(type -t vi)" == file ]] && command vi --version 2>/dev/null | command grep -q 'VIM'; then function vi { command vi -c 'set mouse=a' "$@"; }; fi
if [[ "$(type -t nano)" == file ]]; then function nano {
  if [[ "$(type -P nano)" == /usr/bin/nano && "$(command uname -s)" == Darwin ]]; then
    # macOS nano is Pico: its own xterm mouse decoder is gated on DISPLAY.
    printf '\033[?1006l\033[?1005l'
    DISPLAY="${DISPLAY:-:0}" command nano -m "$@"
  else
    command nano -m "$@"
  fi
}; fi
"#;
pub const POWERSHELL_RC: &str = r#"if (Get-Module -ListAvailable PSReadLine) {
  Import-Module PSReadLine
  $mantashPreviousHandler = (Get-PSReadLineOption).AddToHistoryHandler
  Set-PSReadLineOption -AddToHistoryHandler ({
    param([string]$line)
    [Console]::Write("$([char]27)]777;mantash-cursor;$env:MANTASH_HISTORY_TOKEN;running$([char]7)")
    $decision = if ($mantashPreviousHandler) { $mantashPreviousHandler.Invoke($line) } else { $true }
    $persistable = if ($decision -is [bool]) { $decision } else { "$decision" -eq 'MemoryAndFile' }
    if ($persistable -and $line -and -not $line.StartsWith(' ') -and $line -notmatch '(?i)password|secret|token|credential|api.?key') {
      $command = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($line))
      $directory = if ($PWD.Provider.Name -eq 'FileSystem') { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($PWD.Path)) } else { '' }
      [Console]::Write("$([char]27)]777;mantash;$env:MANTASH_HISTORY_TOKEN;$command;$directory$([char]7)")
    }
    return $decision
  }.GetNewClosure())
}
$mantashCursorEnabled=[bool](Get-Module PSReadLine)
if (Test-Path Function:\prompt) {
  $mantashSavedPrompt = (Get-Item Function:\prompt).ScriptBlock
  Set-Item Function:global:prompt ({
    $mantashPromptText = & $mantashSavedPrompt
    $mantashDirectory = if ($PWD.Provider.Name -eq 'FileSystem') { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($PWD.Path)) } else { '' }
    [Console]::Write("$([char]27)]777;mantash;$env:MANTASH_HISTORY_TOKEN;;$mantashDirectory$([char]7)")
    if ($mantashCursorEnabled) { return "$mantashPromptText$([char]27)]777;mantash-cursor;$env:MANTASH_HISTORY_TOKEN;ready$([char]7)" }
    return $mantashPromptText
  }.GetNewClosure())
}
foreach ($mantashEditor in @('vim','nvim','nano')) {
  $mantashFound=Get-Command $mantashEditor -ErrorAction SilentlyContinue
  if ($mantashFound -and $mantashFound.CommandType -eq 'Application') {
    $mantashEditorPath=$mantashFound.Source
    $mantashEditorFlags=if ($mantashEditor -eq 'nano') { @('-m') } else { @('-c','set mouse=a') }
    Set-Item ("Function:global:"+$mantashEditor) ({ & $mantashEditorPath @mantashEditorFlags @args }.GetNewClosure())
  }
}
"#;

/// CMD expands $E only when drawing its prompt, keeping the marker out of command input.
pub fn cmd_prompt(original: &str, token: &str) -> String {
    format!("{original}$E]777;mantash-cursor;{token};ready$E\\")
}

/// Shell-quote a literal argument without expansion or command substitution.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Write only MantaSH-owned hook files. Nonces are passed through the child environment.
pub fn write_local(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    for (name, text) in [
        (".zshenv", ZSH_ENV),
        (".zshrc", ZSH_RC),
        ("bash.rc", BASH_RC),
        ("powershell.ps1", POWERSHELL_RC),
    ] {
        std::fs::write(directory.join(name), text)?;
    }
    for (filename, suffix) in [(".zprofile", ".zprofile"), (".zlogin", ".zlogin")] {
        std::fs::write(
            directory.join(filename),
            format!(
                "[[ -r \"${{MANTASH_USER_ZDOTDIR:-$HOME}}/{suffix}\" ]] && source \"${{MANTASH_USER_ZDOTDIR:-$HOME}}/{suffix}\"\nZDOTDIR=$MANTASH_INTEGRATION_DIR\n"
            ),
        )?;
    }
    Ok(())
}

/// Start the remote user's shell with ephemeral hooks. SFTP availability is not required.
pub fn remote_command(token: &str) -> String {
    format!(
        r#"export MANTASH_HISTORY_TOKEN={token}
export MANTASH_USER_ZDOTDIR="${{ZDOTDIR:-$HOME}}"
mantash_tmp=$(mktemp -d "${{TMPDIR:-/tmp}}/mantash.XXXXXXXX") || exec "${{SHELL:-/bin/sh}}" -l
export MANTASH_INTEGRATION_DIR="$mantash_tmp"
trap 'rm -f -- "$mantash_tmp/.zshenv" "$mantash_tmp/.zshrc" "$mantash_tmp/bash.rc"; rmdir -- "$mantash_tmp"' EXIT
case "${{SHELL##*/}}" in
zsh) printf '%s' {zsh_env} > "$mantash_tmp/.zshenv"; printf '%s' {zsh_rc} > "$mantash_tmp/.zshrc"; ZDOTDIR="$mantash_tmp" "$SHELL" -i ;;
bash) printf '%s' {bash_rc} > "$mantash_tmp/bash.rc"; "$SHELL" --rcfile "$mantash_tmp/bash.rc" -i ;;
*) "${{SHELL:-/bin/sh}}" -l ;;
esac
"#,
        token = quote(token),
        zsh_env = quote(ZSH_ENV),
        zsh_rc = quote(ZSH_RC),
        bash_rc = quote(BASH_RC)
    )
}
