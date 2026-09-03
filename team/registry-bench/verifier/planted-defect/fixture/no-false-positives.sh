#!/bin/sh
# Exit 0 when report.md names no symbol the patch does not touch.
#
# The patch changes `last_page` and nothing else. A report that also indicts
# `page_starts`, or a symbol that does not exist, is reporting a defect that is
# not there.
#
# This script runs from the READ-ONLY staged grader copy, with the subject's
# scratch dir as cwd. It is never writable by the thing it grades.
set -eu
test -f report.md || exit 1
for absent in page_starts collect_pages normalize_page; do
  if grep -q "$absent" report.md; then
    echo "report.md names $absent, which this patch does not touch" >&2
    exit 1
  fi
done
exit 0
