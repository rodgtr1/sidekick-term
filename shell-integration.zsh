# sidekick shell integration for zsh
# Emits OSC 133 sequences so sidekick knows when commands start and finish.
# This drives the tab notification dot — it fires only when a command
# completes and the shell returns to the prompt, not on every output line.
#
# Add to ~/.zshrc:
#   source ~/.config/sidekick/shell-integration.zsh

_sidekick_precmd() {
    # OSC 133;A = prompt is about to appear (previous command finished)
    printf '\033]133;A\007'
}

_sidekick_preexec() {
    # OSC 133;C = command is about to run
    printf '\033]133;C\007'
}

precmd_functions+=(_sidekick_precmd)
preexec_functions+=(_sidekick_preexec)
