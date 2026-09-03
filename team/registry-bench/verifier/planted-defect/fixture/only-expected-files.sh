#!/bin/sh
# Exit 0 when the scratch dir holds only the fixture's own files plus the two
# the task asked for.
#
# "Write two files and nothing else" is an instruction, and following it is
# evidence about discipline. Checking one hardcoded filename let every OTHER
# stray file through.
set -eu
allowed="pager.rs subject.patch no-false-positives.sh names-the-defect.sh only-expected-files.sh report.md verdict.json"
status=0
for entry in * .[!.]*; do
  [ -e "$entry" ] || continue
  case " $allowed " in
    *" $entry "*) ;;
    *)
      # `.bench` is the harness's own directory, not the subject's output.
      [ "$entry" = ".bench" ] && continue
      echo "unexpected file in the answer directory: $entry" >&2
      status=1
      ;;
  esac
done
exit "$status"
