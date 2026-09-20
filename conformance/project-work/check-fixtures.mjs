#!/usr/bin/env node
// Fixture well-formedness check for conformance/project-work.
//
// This is NOT the contract's validator and NOT the fold. Those are lane W1's,
// in buzz-core, and these fixtures are what they bind to. This script only
// asserts that the fixtures themselves are well formed — every file parses,
// every id has the right shape and length, every tag agrees with its content,
// every sequence's expected output refers to events that exist — so a later
// edit cannot silently break them.
//
// Run: node conformance/project-work/check-fixtures.mjs
// Wired into no CI recipe on purpose (W0 owns this directory alone).

import { readFileSync, readdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FIX = join(HERE, "fixtures");

const SCHEMA = "buzz-project-work/v1";
const KIND = 44249;
const MAX_PLAN_BYTES = 65536;
const MAX_CRITERIA = 64;
const MAX_CONTENT_BYTES = 16384;
const SLUG = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/;
const HEX64 = /^[0-9a-f]{64}$/;
const COMMIT = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const COORD = /^30621:[0-9a-f]{64}:[A-Za-z0-9._-]{1,128}$/;
const TYPES = ["work.declared", "work.assignment_bound", "work.evidence_bound"];
const EVIDENCE_KINDS = ["report", "verdict", "action_result", "ref_observation"];
const PLAN_KEYS = [
  "schema", "id", "status", "title",
  "code_repository", "delivery_ref", "criteria", "retired_criteria",
];

const failures = [];
const fail = (where, message) => failures.push(`${where}: ${message}`);
const check = (cond, where, message) => {
  if (!cond) fail(where, message);
  return cond;
};

// ---------------------------------------------------------------- plan files
// Deliberately line-based, not a YAML parser: the parser is W1's, and a second
// parser here would be a second opinion nobody asked for.
function readPlan(path) {
  const text = readFileSync(path, "utf8");
  const bytes = Buffer.byteLength(text);
  const lines = text.split("\n");
  if (lines[0] !== "---") return { bytes, error: "no opening --- on line 1" };
  const end = lines.indexOf("---", 1);
  if (end < 0) return { bytes, error: "frontmatter is never closed" };
  const fm = lines.slice(1, end);
  const topKeys = [];
  const criteria = [];
  const refusals = [];
  let retired = null;
  let current = null;
  for (const line of fm) {
    if (/^\s*#/.test(line)) {
      const m = line.match(/#\s*REFUSED:\s*(.+)$/);
      if (m) refusals.push(m[1].trim());
      continue;
    }
    const top = line.match(/^([a-z_]+):(.*)$/);
    if (top) {
      topKeys.push(top[1]);
      if (top[1] === "retired_criteria") retired = top[2].trim();
      current = null;
      continue;
    }
    const id = line.match(/^\s+-\s+id:\s*(.*)$/);
    if (id) {
      current = { id: id[1].trim(), accept: null, proof: null };
      criteria.push(current);
      continue;
    }
    const accept = line.match(/^\s+accept:\s*(.*)$/);
    if (accept && current) current.accept = accept[1].trim();
    const proof = line.match(/^\s+proof:\s*(.*)$/);
    if (proof && current) current.proof = proof[1].trim();
  }
  return { text, bytes, topKeys, criteria, retired, refusals };
}

function checkValidPlan(path) {
  const where = `plans/valid/${path.split("/").pop()}`;
  const plan = readPlan(path);
  if (plan.error) return fail(where, plan.error);
  check(plan.bytes <= MAX_PLAN_BYTES, where, `${plan.bytes} bytes exceeds the ${MAX_PLAN_BYTES}-byte ceiling`);
  for (const key of PLAN_KEYS) {
    check(plan.topKeys.includes(key), where, `missing frontmatter key "${key}"`);
  }
  for (const key of plan.topKeys) {
    check(PLAN_KEYS.includes(key), where, `unknown frontmatter key "${key}"`);
  }
  check(plan.criteria.length >= 1, where, "no criteria");
  check(plan.criteria.length <= MAX_CRITERIA, where, `${plan.criteria.length} criteria exceeds ${MAX_CRITERIA}`);
  check(plan.refusals.length === 0, where, "a valid plan must carry no REFUSED marker");
  const seen = new Set();
  const retired = (plan.retired ?? "[]").replace(/[[\]]/g, "").split(",").map((s) => s.trim()).filter(Boolean);
  for (const c of plan.criteria) {
    check(SLUG.test(c.id), where, `criterion id "${c.id}" is not slug grammar`);
    check(Buffer.byteLength(c.id) <= 64, where, `criterion id "${c.id}" exceeds 64 bytes`);
    check(!seen.has(c.id), where, `duplicate criterion id "${c.id}"`);
    seen.add(c.id);
    check(!retired.includes(c.id), where, `criterion id "${c.id}" is both active and retired`);
    check(c.accept !== null && c.accept !== "" && !/^"\s*"$/.test(c.accept), where, `criterion "${c.id}" has empty accept text`);
    check(c.proof !== null, where, `criterion "${c.id}" has no proof`);
    if (c.proof) {
      const ok =
        c.proof === "{kind: review}" ||
        c.proof === "{kind: git-ref}" ||
        /^\{kind: action, name: [a-z0-9-]+, step: [a-z0-9-]+\}$/.test(c.proof);
      check(ok, where, `criterion "${c.id}" has a proof that is not one of the three forms: ${c.proof}`);
    }
  }
  return seen;
}

function checkInvalidPlan(path) {
  const name = path.split("/").pop();
  const where = `plans/invalid/${name}`;
  const plan = readPlan(path);
  if (plan.error) return fail(where, plan.error);
  check(plan.refusals.length === 1, where, "an invalid plan must carry exactly one # REFUSED: marker naming its reason");
  // Assert the defect is still present, so nobody repairs a fixture by hand
  // and leaves the refusal marker lying about what the file is.
  const ids = plan.criteria.map((c) => c.id);
  const retired = (plan.retired ?? "[]").replace(/[[\]]/g, "").split(",").map((s) => s.trim()).filter(Boolean);
  switch (name) {
    case "duplicate-id.md":
      check(new Set(ids).size < ids.length, where, "no duplicate criterion id present");
      break;
    case "recycled-retired-id.md":
      check(ids.some((id) => retired.includes(id)), where, "no active/retired overlap present");
      break;
    case "unknown-key.md":
      check(plan.topKeys.some((k) => !PLAN_KEYS.includes(k)), where, "no unknown frontmatter key present");
      break;
    case "empty-accept.md":
      check(plan.criteria.some((c) => c.accept === null || /^"?\s*"?$/.test(c.accept)), where, "no empty accept present");
      break;
    case "absolute-path-in-action-name.md":
      check(plan.criteria.some((c) => (c.proof ?? "").includes("/")), where, "no path-shaped action name present");
      break;
    case "oversize.md":
      check(plan.bytes > MAX_PLAN_BYTES, where, `${plan.bytes} bytes does not exceed the ceiling`);
      break;
    default:
      fail(where, "unknown invalid-plan fixture: add its defect assertion to check-fixtures.mjs");
  }
}

// -------------------------------------------------------------- work records
const KEYS = {
  "work.declared": ["workId", "goalRef", "responsibleActor", "planRef", "supersedes"],
  "work.assignment_bound": ["declarationRef", "criterionIds", "assignmentRef", "replacesBinding"],
  "work.evidence_bound": ["declarationRef", "criterionIds", "artifactCommit", "evidenceRefs", "completionRef"],
};

function checkEvent(event, where) {
  const before = failures.length;
  check(event && typeof event === "object", where, "not an object");
  if (!event || typeof event !== "object") return false;
  check(HEX64.test(event.id ?? ""), where, "id is not 64 lowercase hex");
  check(HEX64.test(event.pubkey ?? ""), where, "pubkey is not 64 lowercase hex");
  check(Number.isInteger(event.created_at), where, "created_at is not an integer");
  check(event.kind === KIND, where, `kind is ${event.kind}, not ${KIND}`);
  check(Buffer.byteLength(event.content ?? "") <= MAX_CONTENT_BYTES, where, "content exceeds 16 KiB");
  let p;
  try {
    p = JSON.parse(event.content);
  } catch (e) {
    fail(where, `content is not JSON: ${e.message}`);
    return false;
  }
  check(p.schema === SCHEMA, where, `schema is "${p.schema}"`);
  check(UUID.test(p.sessionRef ?? ""), where, "sessionRef is not a uuid");
  check(HEX64.test(p.genesisRef ?? ""), where, "genesisRef is not 64 hex");
  check(COORD.test(p.projectRef ?? ""), where, `projectRef is not a canonical coordinate: ${p.projectRef}`);
  check(TYPES.includes(p.type), where, `type is "${p.type}"`);
  check(Object.keys(p).length === 6, where, `payload has ${Object.keys(p).length} keys, not 6`);

  const tags = event.tags ?? [];
  check(tags.length === 6, where, `${tags.length} tags, not 6`);
  check(tags.every((t) => Array.isArray(t) && t.length === 2), where, "a tag is not a two-field tag");
  const expect = [
    ["h", UUID],
    ["d", p.sessionRef],
    ["a", p.projectRef],
    ["pwk-v", SCHEMA],
    ["pwk-genesis", p.genesisRef],
    ["pwk-type", p.type],
  ];
  expect.forEach(([name, want], i) => {
    const tag = tags[i];
    if (!Array.isArray(tag) || tag.length !== 2) return;
    check(tag[0] === name, where, `tag ${i} is "${tag[0]}", expected "${name}"`);
    if (want instanceof RegExp) check(want.test(tag[1]), where, `tag "${name}" value is malformed`);
    else check(tag[1] === want, where, `tag "${name}" does not match content (${tag[1]} vs ${want})`);
  });

  const body = p.body;
  if (body && typeof body === "object" && KEYS[p.type]) {
    const want = KEYS[p.type];
    check(Object.keys(body).length === want.length, where, `body has keys [${Object.keys(body)}], expected [${want}]`);
    for (const k of want) check(k in body, where, `body is missing "${k}" (nullable keys are written as null, never absent)`);
  } else {
    fail(where, "body is not an object");
    return false;
  }
  if (p.type === "work.declared") {
    check(UUID.test(body.workId ?? ""), where, "workId is not a uuid");
    check(HEX64.test(body.goalRef ?? ""), where, "goalRef is not 64 hex");
    check(HEX64.test(body.responsibleActor ?? ""), where, "responsibleActor is not 64 hex");
    const r = body.planRef ?? {};
    check(Object.keys(r).length === 3, where, "planRef must have exactly repository, commit, path");
    check(SLUG.test(r.repository ?? ""), where, `planRef.repository is not a repo id: ${r.repository}`);
    check(COMMIT.test(r.commit ?? ""), where, `planRef.commit is not a full 40/64 hex commit: ${r.commit}`);
    check(typeof r.path === "string" && r.path.startsWith("plans/") && !r.path.includes(".."), where, `planRef.path is not a plans/ path: ${r.path}`);
    check(Array.isArray(body.supersedes) && body.supersedes.every((x) => HEX64.test(x)), where, "supersedes is not a list of event ids");
    check((body.supersedes ?? []).length <= 8, where, "supersedes exceeds 8 entries");
    check(!(body.supersedes ?? []).includes(event.id), where, "a declaration cannot supersede its own event id");
  } else {
    check(HEX64.test(body.declarationRef ?? ""), where, "declarationRef is not 64 hex");
    const ids = body.criterionIds;
    check(Array.isArray(ids) && ids.length >= 1, where, "criterionIds must name at least one criterion");
    if (Array.isArray(ids)) {
      check(ids.length <= MAX_CRITERIA, where, `criterionIds exceeds ${MAX_CRITERIA}`);
      check(new Set(ids).size === ids.length, where, "criterionIds repeats an id");
      for (const id of ids) check(SLUG.test(id), where, `criterion id "${id}" is not slug grammar`);
    }
  }
  if (p.type === "work.assignment_bound") {
    check(HEX64.test(body.assignmentRef ?? ""), where, "assignmentRef is not 64 hex");
    check(body.replacesBinding === null || HEX64.test(body.replacesBinding ?? ""), where, "replacesBinding must be null or 64 hex");
  }
  if (p.type === "work.evidence_bound") {
    check(COMMIT.test(body.artifactCommit ?? ""), where, `artifactCommit is not 40/64 hex: ${body.artifactCommit}`);
    const refs = body.evidenceRefs;
    check(Array.isArray(refs) && refs.length >= 1, where, "evidenceRefs must carry at least one reference");
    check(!Array.isArray(refs) || refs.length <= 32, where, "evidenceRefs exceeds 32 entries");
    for (const r of refs ?? []) {
      check(r && Object.keys(r).length === 2, where, "an evidence ref must be exactly {kind, eventId}");
      check(EVIDENCE_KINDS.includes(r?.kind), where, `evidence kind "${r?.kind}" is not one of ${EVIDENCE_KINDS.join("|")}`);
      check(HEX64.test(r?.eventId ?? ""), where, "an evidence eventId is not 64 hex");
    }
    check(body.completionRef === null || HEX64.test(body.completionRef ?? ""), where, "completionRef must be null or 64 hex");
  }
  return failures.length === before;
}

function checkRecords() {
  for (const name of readdirSync(join(FIX, "records/valid")).sort()) {
    const where = `records/valid/${name}`;
    const event = JSON.parse(readFileSync(join(FIX, "records/valid", name), "utf8"));
    checkEvent(event, where);
  }
  for (const name of readdirSync(join(FIX, "records/invalid")).sort()) {
    const where = `records/invalid/${name}`;
    const file = JSON.parse(readFileSync(join(FIX, "records/invalid", name), "utf8"));
    if (!check(typeof file.refusal === "string" && file.refusal.length > 0, where, "no refusal reason")) continue;
    if (!check(file.event && typeof file.event === "object", where, "no event")) continue;
    // The shape checks above must reject it: a refusal fixture that passes is
    // a fixture that stopped testing anything.
    const before = failures.length;
    checkEvent(file.event, where);
    if (failures.length === before) fail(where, "this refusal fixture passes every shape check — it no longer refuses anything");
    else failures.length = before;
  }
}

// ------------------------------------------------------------------ sequences
function checkSequences(planCriteria) {
  for (const name of readdirSync(join(FIX, "sequences")).sort()) {
    const dir = join(FIX, "sequences", name);
    const where = `sequences/${name}`;
    const events = JSON.parse(readFileSync(join(dir, "events.json"), "utf8"));
    const fold = JSON.parse(readFileSync(join(dir, "expected-fold.json"), "utf8"));
    const inputs = JSON.parse(readFileSync(join(dir, "inputs.json"), "utf8"));
    check(Array.isArray(events) && events.length > 0, where, "events.json is not a non-empty array");
    const ids = new Set();
    const times = new Set();
    for (const [i, event] of events.entries()) {
      checkEvent(event, `${where}/events[${i}]`);
      check(Number.isInteger(event.created_at), `${where}/events[${i}]`, "no fixed timestamp");
      times.add(`${event.id}:${event.created_at}`);
      ids.add(event.id);
    }
    check(times.size === events.length, where, "two events share an id and timestamp");
    for (const pubkey of inputs.mayLead ?? []) check(HEX64.test(pubkey), where, "a mayLead pubkey is not 64 hex");
    for (const [key, rel] of Object.entries(inputs.planBlobs ?? {})) {
      check(/^[a-z0-9-]+@(?:[0-9a-f]{40}|[0-9a-f]{64}):plans\/[a-z0-9-]+\.md$/.test(key), where, `plan blob key is malformed: ${key}`);
      check(existsSync(resolve(dir, rel)), where, `plan blob fixture is missing: ${rel}`);
    }
    check(fold.schema === "buzz-project-work-coverage/v1", where, "expected-fold schema is wrong");
    check(Array.isArray(fold.declarations) && fold.declarations.length > 0, where, "no declarations in expected fold");
    for (const d of fold.declarations) {
      check(ids.has(d.declarationRef), where, `expected fold names declaration ${d.declarationRef} which is not in events.json`);
      check(["head", "superseded", "stale", "conflict"].includes(d.state), where, `unknown declaration state "${d.state}"`);
      check(typeof d.coverageComplete === "boolean", where, "coverageComplete is not a boolean");
      check(!("missionTerminal" in d) && !("mission" in fold), where, "coverage output must not carry a mission terminal field: two questions, never merged");
      if (d.state === "head" || d.state === "stale") {
        check(d.criteria.length > 0, where, "a head declaration must project its criteria");
      } else {
        check(d.criteria.length === 0, where, `a ${d.state} declaration projects no criteria`);
      }
      for (const c of d.criteria) {
        check(SLUG.test(c.criterionId), where, `criterion id "${c.criterionId}" is not slug grammar`);
        check(planCriteria.has(c.criterionId), where, `criterion "${c.criterionId}" is not in the plan fixture`);
        check(["open", "covered", "stale", "unknown"].includes(c.status), where, `unknown criterion status "${c.status}"`);
        check((c.status === "covered") === (c.evidence.length > 0), where, `criterion "${c.criterionId}" is ${c.status} with ${c.evidence.length} evidence refs`);
        check((c.status === "open" || c.status === "covered") === (c.reason === null), where, `criterion "${c.criterionId}" is ${c.status}: a reason is required unless it is open or covered`);
        for (const e of c.evidence) check(EVIDENCE_KINDS.includes(e.kind) && HEX64.test(e.eventId), where, "malformed evidence ref in expected fold");
      }
      if (d.coverageComplete) {
        check(d.state === "head", where, "coverageComplete on a declaration that is not the head");
        check(d.criteria.every((c) => c.status === "covered"), where, "coverageComplete with an uncovered criterion");
        check(fold.conflicts.length === 0, where, "coverageComplete while a conflict is recorded");
      }
    }
    for (const x of fold.excluded ?? []) {
      check(ids.has(x.eventId), where, `excluded event ${x.eventId} is not in events.json`);
      check(typeof x.code === "string" && x.code.length > 0, where, "an exclusion carries no code");
    }
    for (const c of fold.conflicts ?? []) {
      check(Array.isArray(c.heads) && c.heads.length >= 2, where, "a conflict must name at least two heads");
      for (const h of c.heads) check(ids.has(h), where, `conflict head ${h} is not in events.json`);
    }
  }
}

// ---------------------------------------------------------------------- main
const planCriteria = checkValidPlan(join(FIX, "plans/valid/kettle.md")) ?? new Set();
for (const name of readdirSync(join(FIX, "plans/invalid")).sort()) {
  checkInvalidPlan(join(FIX, "plans/invalid", name));
}
checkRecords();
checkSequences(planCriteria);

if (failures.length > 0) {
  console.error(`conformance/project-work: ${failures.length} fixture problem(s)`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log("conformance/project-work: fixtures are well formed");
