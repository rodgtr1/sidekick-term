# sidekick shell integration for zsh
# Emits VTE termprop OSC sequences so sidekick knows when commands start and finish.
# This drives the tab notification dot — it fires only when a command
# completes and the shell returns to the prompt, not on every output line.
#
# Add to ~/.zshrc:
#   source ~/.config/sidekick/shell-integration.zsh

_sidekick_precmd() {
    local code="$?"
    # Exit status of the command that just finished — read by sidekick at
    # precmd time for long-command desktop notifications.
    printf '\033]666;vte.ext.sidekick.exit=%s\033\\' "$code"
    # VTE OSC 666: signal vte.shell.precmd (shell is about to show the prompt)
    printf '\033]666;vte.shell.precmd!\033\\'
}

_sidekick_preexec() {
    # VTE OSC 666: signal vte.shell.preexec (shell is about to execute a command)
    printf '\033]666;vte.shell.preexec!\033\\'
}

sidekick_agent_status() {
    local errsv="$?"
    case "$1" in
        busy|working|running)
            printf '\033]666;vte.ext.sidekick.agent=busy\033\\'
            ;;
        ready|prompt|waiting|needs-user|needs_user)
            printf '\033]666;vte.ext.sidekick.agent=ready\033\\'
            ;;
        done|finished|complete)
            printf '\033]666;vte.ext.sidekick.agent=done\033\\'
            ;;
        idle|clear|reset)
            printf '\033]666;vte.ext.sidekick.agent=idle\033\\'
            ;;
        *)
            printf 'usage: sidekick_agent_status busy|ready|done|idle\n' >&2
            return 2
            ;;
    esac
    return $errsv
}

precmd_functions+=(_sidekick_precmd)
preexec_functions+=(_sidekick_preexec)
