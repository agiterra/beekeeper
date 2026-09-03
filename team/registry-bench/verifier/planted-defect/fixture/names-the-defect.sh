#!/bin/sh
# Exit 0 when report.md names `last_page` AS A DEFECT.
#
# A bare `fileContains: last_page` passed on any mention — including a sentence
# clearing the function. This requires the mention and rejects a report whose
# only verdict about it is that it is fine.
set -eu
test -f report.md || { echo "no report.md" >&2; exit 1; }
grep -q "last_page" report.md || {
  echo "report.md never names last_page" >&2; exit 1; }
# A clearing verdict anywhere in the report is not a defect report.
if grep -qiE "no (real )?defects?|looks (fine|correct)|is correct|nothing wrong" report.md; then
  echo "report.md clears the code it was asked to indict" >&2
  exit 1
fi
exit 0
