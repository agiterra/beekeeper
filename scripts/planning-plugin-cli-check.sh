#!/usr/bin/env bash
# planning-plugin-cli-check.sh <relay-ws-url> — `bee planning` through the
# relay's Planning plugin, as two disposable members of one project.
#
# Run by scripts/planning-plugin-check.sh against a relay loaded with
# target/plugins/registry.test.toml (two providers of beekeeper.planning/1,
# so every call names --provider). Keys are generated here and live only in
# this process's environment.
set -euo pipefail

RELAY="${1:?usage: planning-plugin-cli-check.sh <relay-ws-url>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BEE="${ROOT}/target/debug/bee"
OWNER_KEY="$(openssl rand -hex 32)"
MEMBER_KEY="$(openssl rand -hex 32)"

as() {
  local key="$1"; shift
  BEEKEEPER_RELAY_URL="${RELAY}" BEEKEEPER_PRIVATE_KEY="${key}" BEEKEEPER_AUTH_TAG="" "${BEE}" "$@"
}
py() { python3 -c "import json,sys; d=json.load(sys.stdin); $1"; }
step() { echo "STEP $*"; }

DTAG="planning-cli-$$-$(date +%s)"
REPO="$(as "${OWNER_KEY}" repos create --id "${DTAG}" --name "${DTAG}")"
OWNER_PUB="$(echo "${REPO}" | py 'import re; print(re.search(r"owner=([0-9a-f]{64})", d["link"]).group(1))')"
as "${OWNER_KEY}" projects create "${DTAG}" --repo "${DTAG}" --name "CLI ${DTAG}" >/dev/null
PROJECT="30621:${OWNER_PUB}:${DTAG}"
# The member's pubkey, read back the same way from a repo of their own.
MEMBER_REPO="$(as "${MEMBER_KEY}" repos create --id "${DTAG}-m" --name "${DTAG}-m")"
MEMBER_PUB="$(echo "${MEMBER_REPO}" | py 'import re; print(re.search(r"owner=([0-9a-f]{64})", d["link"]).group(1))')"
# A CLI-made project is private: only its roster may read or write it.
as "${OWNER_KEY}" projects add-member "${DTAG}" --pubkey "${MEMBER_PUB}" --role collaborator >/dev/null
step "project ${PROJECT}"

step "providers"
as "${MEMBER_KEY}" planning providers --project "${PROJECT}" \
  | py 'names=sorted(p["name"] for p in d["providers"]); assert names==["planning","planning-fixture"], names; print(names)'

step "a stranger is refused by the relay before the plugin runs"
if out="$(as "$(openssl rand -hex 32)" planning read --project "${PROJECT}" --provider planning 2>&1)"; then
  echo "expected 403, got: ${out}"; exit 1
fi
echo "${out}" | grep -q "403" || { echo "unexpected: ${out}"; exit 1; }

step "two providers and no --provider is an error that names both"
if out="$(as "${MEMBER_KEY}" planning read --project "${PROJECT}" 2>&1)"; then
  echo "expected a refusal, got: ${out}"; exit 1
fi
echo "${out}" | grep -q "planning, planning-fixture" || { echo "refusal did not list providers: ${out}"; exit 1; }

step "member creates an issue through the plugin"
CREATED="$(as "${MEMBER_KEY}" planning create --project "${PROJECT}" --provider planning \
  --title "Filed from the CLI" --status in_progress --priority high --description "via WASM")"
echo "${CREATED}" | py 'assert len(d["published"])==6, d["published"]; assert d["failed"]==[], d["failed"]; print(d["result"]["key"])'
KEY="$(echo "${CREATED}" | py 'print(d["result"]["key"])')"

step "owner reads it through the plugin: created by the member, In Progress"
MEMBER_HEX="${MEMBER_PUB}"
as "${OWNER_KEY}" planning read --project "${PROJECT}" --provider planning \
  | py "i=[x for x in d['result']['issues'] if x['key']=='${KEY}'][0]; assert i['status']['id']=='in_progress', i; assert i['priority']['id']=='high', i; assert i['createdBy']=='${MEMBER_HEX}', i; assert i['createdBy']!='${OWNER_PUB}'; print('ok', i['createdBy'][:16])"

step "owner cancels it through the plugin: two signed events, both the owner's"
as "${OWNER_KEY}" planning status --project "${PROJECT}" --provider planning --issue "${KEY}" --status canceled \
  | py 'assert [p["group"] for p in d["published"]]==["status","status"], d; print("published", len(d["published"]))'

step "bee todos (no plugin) folds the same state"
LIST="${KEY%%/*}"
ITEM="${KEY##*/}"
as "${MEMBER_KEY}" --format compact todos show --project "${PROJECT}" "${LIST}" \
  | py "i=[x for x in d['completed'] if x['id']=='${ITEM}'][0]; assert i['status']=='canceled', i; print('todos show:', i['status'])"

step "activity names both members"
as "${MEMBER_KEY}" planning activity --project "${PROJECT}" --provider planning --issue "${KEY}" \
  | py "a={e['actor'] for e in d['result']['entries']}; assert '${OWNER_PUB}' in a and '${MEMBER_HEX}' in a, a; print(len(d['result']['entries']), 'entries')"

step "the replacement provider answers the same command, and refuses writes honestly"
as "${MEMBER_KEY}" planning read --project "${PROJECT}" --provider planning-fixture \
  | py 'assert d["result"]["issues"][0]["title"]=="Served by the fixture provider"; print("fixture ok")'
if out="$(as "${MEMBER_KEY}" planning title --project "${PROJECT}" --provider planning-fixture --issue fixture/1 --title x 2>&1)"; then
  echo "expected unsupported, got: ${out}"; exit 1
fi
echo "${out}" | grep -q "read-only" || { echo "unexpected refusal: ${out}"; exit 1; }

step "generic invoke reaches the fault plugin and reports its trap"
if out="$(as "${MEMBER_KEY}" plugins invoke test-faults spin --project "${PROJECT}" 2>&1)"; then
  echo "expected a trap, got: ${out}"; exit 1
fi
echo "${out}" | grep -q "fuel" || { echo "unexpected: ${out}"; exit 1; }

echo "CLI CHECK PASSED"
