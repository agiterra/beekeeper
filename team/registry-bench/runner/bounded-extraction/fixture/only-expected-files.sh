#!/bin/sh
# See verifier/planted-defect/fixture/only-expected-files.sh — a general
# property, not one hardcoded filename.
set -eu
allowed="access.log log-untouched.sh under-two-minutes.sh only-expected-files.sh answer.json .access_log.sha256"
status=0
for entry in * .[!.]*; do
  [ -e "$entry" ] || continue
  case " $allowed " in
    *" $entry "*) ;;
    *)
      [ "$entry" = ".bench" ] && continue
      echo "unexpected file in the answer directory: $entry" >&2
      status=1
      ;;
  esac
done
exit "$status"
