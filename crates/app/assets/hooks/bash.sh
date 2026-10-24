# Ngambang: tell the island when a long command finishes.
# Add to ~/.bashrc (Git Bash, WSL, MSYS2):
#   eval "$(ngambang.exe hook bash)"
__ngb_threshold={{THRESHOLD}}
__ngb_ignore=" {{IGNORE}} "
__ngb_armed=1
__ngb_preexec() {
  [ "$__ngb_armed" = 1 ] || return
  case "$BASH_COMMAND" in __ngb_precmd*) return ;; esac
  __ngb_armed=0
  __ngb_cmd=$BASH_COMMAND
  __ngb_start=$SECONDS
}
__ngb_precmd() {
  local code=$?
  if [ -n "$__ngb_start" ]; then
    local d=$((SECONDS - __ngb_start)) first=${__ngb_cmd%% *}
    if [ "$d" -ge "$__ngb_threshold" ] && [[ "$__ngb_ignore" != *" $first "* ]]; then
      if [ "$code" = 0 ]; then
        ngambang.exe done "$__ngb_cmd" --ok --duration "$d" --quiet
      else
        ngambang.exe done "$__ngb_cmd" --fail --code "$code" --duration "$d" --quiet
      fi
    fi
    unset __ngb_start
  fi
  __ngb_armed=1
  return $code
}
trap '__ngb_preexec' DEBUG
PROMPT_COMMAND="__ngb_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
