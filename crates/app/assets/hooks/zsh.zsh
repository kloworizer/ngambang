# Ngambang: tell the island when a long command finishes.
# Add to ~/.zshrc (WSL, MSYS2):
#   eval "$(ngambang.exe hook zsh)"
autoload -Uz add-zsh-hook
typeset -g __ngb_threshold={{THRESHOLD}}
typeset -g __ngb_ignore=" {{IGNORE}} "
__ngb_preexec() { __ngb_cmd=$1; __ngb_start=$SECONDS }
__ngb_precmd() {
  local code=$?
  if [[ -n $__ngb_start ]]; then
    local d=$(( SECONDS - __ngb_start )) first=${__ngb_cmd%% *}
    if (( d >= __ngb_threshold )) && [[ $__ngb_ignore != *" $first "* ]]; then
      if (( code == 0 )); then
        ngambang.exe done "$__ngb_cmd" --ok --duration $d --quiet
      else
        ngambang.exe done "$__ngb_cmd" --fail --code $code --duration $d --quiet
      fi
    fi
    unset __ngb_start
  fi
}
add-zsh-hook preexec __ngb_preexec
add-zsh-hook precmd __ngb_precmd
