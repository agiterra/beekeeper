#!/bin/sh
# Beekeeper execution boundary: this execution's `mktemp`.
#
# macOS mktemp puts default (no template) and `-t prefix` names under the
# per-user Darwin temp directory, ignoring TMPDIR; that shared directory is
# outside the project's boundary. Any -p/--tmpdir switches the base to the
# given directory, so add `-p "$TMPDIR"` (this execution's private temp) when
# the caller gave none. Relative templates stay relative to the working
# directory. `/usr/bin/mktemp` named absolutely is not rerouted.
real=/usr/bin/mktemp
[ -n "$TMPDIR" ] || exec "$real" "$@"
has_p=0 has_t=0 npos=0 expect=
for a in "$@"; do
  if [ -n "$expect" ]; then expect=; continue; fi
  if [ "$npos" = done ]; then npos=1; continue; fi
  case $a in
    --) npos=done ;;
    --tmpdir|--tmpdir=*) has_p=1 ;;
    --*) ;;
    -?*)
      rest=${a#-}
      while [ -n "$rest" ]; do
        c=${rest%"${rest#?}"}; rest=${rest#?}
        case $c in
          p) has_p=1; [ -z "$rest" ] && expect=1; rest= ;;
          t) has_t=1; [ -z "$rest" ] && expect=1; rest= ;;
        esac
      done ;;
    *) npos=1 ;;
  esac
done
[ "$npos" = done ] && npos=0
if [ $has_p = 0 ] && { [ $npos = 0 ] || [ $has_t = 1 ]; }; then
  if [ $npos != 0 ]; then
    # -p makes templates relative to it: pin relative ones to the cwd.
    n=$#; expect=; ddash=0
    while [ $n -gt 0 ]; do
      a=$1; shift; n=$((n-1))
      if [ -n "$expect" ]; then expect=; set -- "$@" "$a"; continue; fi
      case $ddash$a in
        0--) ddash=1 ;;
        0--*) ;;
        0-?*) case $a in -*[tp]) expect=1 ;; esac ;;
        *) case $a in /*) ;; *) a="$PWD/$a" ;; esac ;;
      esac
      set -- "$@" "$a"
    done
  fi
  exec "$real" -p "$TMPDIR" "$@"
fi
exec "$real" "$@"
