# sidekick shell integration for zsh
# Emits VTE termprop OSC sequences so sidekick knows when commands start and finish.
# This drives the tab notification dot — it fires only when a command
# completes and the shell returns to the prompt, not on every output line.
#
# Add to ~/.zshrc:
#   source ~/.config/sidekick/shell-integration.zsh

_sidekick_precmd() {
    # VTE OSC 666: signal vte.shell.precmd (shell is about to show the prompt)
    printf '\033]666;vte.shell.precmd!\033\\'
}

_sidekick_preexec() {
    # VTE OSC 666: signal vte.shell.preexec (shell is about to execute a command)
    printf '\033]666;vte.shell.preexec!\033\\'
}

precmd_functions+=(_sidekick_precmd)
preexec_functions+=(_sidekick_preexec)
