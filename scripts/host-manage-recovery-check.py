#!/usr/bin/env python3
"""What happens after the first thing goes wrong in a Manage Host setup.

The scenarios behind review findings B1-B6 and M1, each through public operations:
`bee host manage`, and the window's real page (host-manage-page-step.mjs) for the
actions a person takes. Same isolation as host-manage-check.py, whose harness this
reuses: its own relay, Host and generated keys, a scripted runtime, no model spend.

Usage: scripts/host-manage-recovery-check.py [--only R1,R2] [--evidence DIR]
"""
import argparse
import importlib.util
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import time
import traceback
import uuid

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("host_manage_check", HERE / "host-manage-check.py")
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)
OVERALL_LIMIT_SECS = 2700


def decode_nsec(key):
    """A secret key as hex, from either form. Bech32 by hand: this script imports nothing."""
    if not key.startswith("nsec1"):
        return key
    charset = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    bits, value, out = 0, 0, bytearray()
    for char in key[5:-6]:
        value = (value << 5) | charset.index(char)
        bits += 5
        while bits >= 8:
            bits -= 8
            out.append((value >> bits) & 0xff)
    return out.hex()


class Recovery(base.Harness):
    def page(self, setup, click=None):
        env = dict(self.env, MENUBAR_BIN=str(base.MENUBAR), BEEKEEPER_BEE=str(base.BEE), NODE_PATH=str(base.ROOT / "desktop/node_modules"),
                   PLAYWRIGHT_BROWSERS_PATH=str(pathlib.Path(os.environ["HOME"]) / "Library/Caches/ms-playwright"))
        if os.environ.get("HM_PAGE_FILE"):
            env["HM_PAGE_FILE"] = os.environ["HM_PAGE_FILE"]
        payload = {"setup": setup, "keyFile": str(self.key_file)}
        if click:
            payload["click"] = click
        result = subprocess.run(["node", str(HERE / "host-manage-page-step.mjs"), json.dumps(payload)], env=env, cwd=base.ROOT / "desktop", capture_output=True, text=True, timeout=260)
        view = json.loads(result.stdout.strip().splitlines()[-1]) if result.stdout.strip() else {"error": result.stderr[-400:]}
        self.save(f"page-{setup[:8]}-{click or 'read'}-{int(time.time())}.json", view)
        return view

    def fresh(self, name, role="lead", access=None):
        slug = f"r{uuid.uuid4().hex[:8]}"
        channel = self.channel(slug)
        project, code, agents, model = self.project(slug, channel)
        code_exit, _, message = self.manage("trust", "--project", project, "--checkout", code, "--agents-repo", agents)
        if code_exit:
            raise RuntimeError(message)
        code_exit, planned, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, role, name, model, access=access))
        if code_exit:
            raise RuntimeError(f"plan: {message}")
        return {"id": planned["setupId"], "project": project, "channel": channel, "agents": agents, "code": code, "model": model, "name": name}

    def started(self, name):
        made = self.fresh(name)
        code, applied, message = self.manage("apply", "--setup", made["id"])
        if code:
            raise RuntimeError(f"apply: {message}")
        made["actor"] = applied["agentPubkey"]
        return made

    def state(self, setup_id):
        return self.setup_row(setup_id)["effectiveState"]

    def creates(self, channel):
        rows = self.read([44221], f"commands-{uuid.uuid4().hex[:6]}.json", channel=channel, extra={"authors": [self.owner]})
        return [r for r in rows if json.loads(r["content"])["action"]["type"] == "session.create"]

    def created_receipts(self, channel):
        rows = self.read([44224], f"receipts-{uuid.uuid4().hex[:6]}.json", channel=channel, extra={"authors": [self.provider]})
        return [json.loads(r["content"]) for r in rows if json.loads(r["content"]).get("status") == "created"]

    def provider_down(self):
        self.raw("host", "stop")
        self.wait_for("provider stopped", lambda: self.inventory()["host"]["provider"]["state"] == "notSupervised", 40)

    def provider_up(self):
        self.raw("host", "start")
        self.wait_for("provider live", lambda: self.inventory()["host"]["provider"]["state"] == "live", 60)
        self.wait_for("catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)

    def credential_paths(self):
        return self.work / "acme.json", self.work / "acme.json.away"

    def kill_when(self, setup_id, predicate, verb="apply", extra=()):
        process = subprocess.Popen([str(base.BEE), "host", "manage", verb, "--setup", setup_id, "--owner-key-file", str(self.key_file), *extra], env=self.env, cwd=self.work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            try:
                journal = self.journal(setup_id)
            except (ValueError, OSError):
                journal = None
            if journal and predicate(journal):
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                return True
            if process.poll() is not None:
                return False
            time.sleep(0.002)
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        return False

    @staticmethod
    def at_step(step):
        return lambda journal: journal["steps"] and journal["steps"][-1]["step"] == step and journal["steps"][-1]["state"] == "running"

    def mention(self, actor, channel, text):
        return json.loads(self.owner_bee("messages", "send", "--channel", channel, "--mention", actor, "--content", text))["event_id"]

    def operation_file(self, operation):
        return self.home / ".local/state/buzz/host/management/operations" / f"{operation}.json"

    # ── R1: a signed refusal closes an attempt; an unknown outcome does not ──
    def r1_refusal_cli(self):
        path, away = self.credential_paths()
        made = self.fresh("R1 cli")
        shutil.move(path, away)
        code, applied, message = self.manage("apply", "--setup", made["id"])
        shutil.move(away, path)
        first_id = self.journal(made["id"])["startOperation"]
        self.check("R1 cli: a signed provider refusal names the provider's own code", code != 0 and "MODEL_AUTH_UNAVAILABLE" == (applied or {}).get("lastRefusal", {}).get("code"), (applied or {}).get("lastRefusal"))
        self.check("R1 cli: the refusal is definite, not an unknown outcome", (applied or {}).get("lastRefusal", {}).get("uncertain") is False, (applied or {}).get("lastRefusal"))
        held = self.journal(made["id"]).get("pendingAction")
        self.check("R1 cli: a fresh start id is persisted only after that refusal", held and held["action"] == "start" and held["operationId"] != first_id, held)
        code, applied, message = self.manage("apply", "--setup", made["id"])
        self.check("R1 cli: after the cause is fixed, retry reaches running", code == 0 and applied["state"] == "running", f"{message} {applied and applied.get('lastRefusal')}")
        self.check("R1 cli: one identity through refusal and retry", len(self.definitions("R1 cli")) == 1, "")

    def r1_refusal_page(self):
        path, away = self.credential_paths()
        made = self.fresh("R1 page")
        shutil.move(path, away)
        view = self.page(made["id"], "Apply")
        shutil.move(away, path)
        after = view.get("after", {})
        self.check("R1 page: the window shows the provider's code", "MODEL_AUTH_UNAVAILABLE" in (after.get("refusal") or ""), view)
        self.check("R1 page: the window says retry starts a new attempt", "new attempt" in (after.get("refusal") or ""), after.get("refusal"))
        view = self.page(made["id"], "Retry")
        self.check("R1 page: Retry in the window reaches running", (view.get("after", {}).get("badge") or "") == "running", view)

    def r1_unknown_keeps_id(self):
        made = self.started("R1 unknown")
        self.manage("stop", "--setup", made["id"])
        self.provider_down()
        code, journal, message = self.manage("start", "--setup", made["id"], timeout=120)
        held = self.journal(made["id"]).get("pendingAction")
        last = (journal or {}).get("lastRefusal") or {}
        self.check("R1 unknown: no receipt is an uncertain outcome, not a refusal", code != 0 and last.get("uncertain") is True, f"{code} {message} {last}")
        self.provider_up()
        before = (held or {}).get("operationId")
        # The provider answers a command it missed some time after it returns. Until
        # then the honest answer stays "unconfirmed", on the same id, however often
        # a person retries.
        deadline = time.monotonic() + 150
        seen = set()
        while True:
            code, journal, message = self.manage("apply", "--setup", made["id"])
            held_now = self.journal(made["id"]).get("pendingAction")
            if held_now:
                seen.add(held_now["operationId"])
            if code == 0 or time.monotonic() > deadline:
                break
            time.sleep(5)
        self.check("R1 unknown: no retry minted a new id while the outcome was unknown", seen <= {before}, seen)
        self.save("r1-unknown-final.json", journal or {})
        receipts = [r for r in self.created_receipts(made["channel"]) if r.get("commandId") == before]
        self.check("R1 unknown: retry reuses the exact id and reaches running", code == 0 and journal["state"] == "running", f"{message} {journal and journal.get('lastRefusal')}")
        self.check("R1 unknown: that id produced exactly one created receipt", len(receipts) == 1, f"{before} {receipts}")
        self.transitions(made, "R1 unknown", page=False)

    # ── R2: Retry does what was asked ──
    def r2_stop_retry(self, page):
        label = "page" if page else "cli"
        made = self.started(f"R2 {label}")
        creates_before = len(self.creates(made["channel"]))
        self.provider_down()
        code, journal, message = self.manage("stop", "--setup", made["id"], timeout=120)
        held = self.journal(made["id"]).get("pendingAction")
        self.check(f"R2 {label}: an unconfirmed stop is recorded with its id", code != 0 and held and held["action"] == "stop", f"{code} {message} {held}")
        code, _, message = self.manage("start", "--setup", made["id"])
        self.check(f"R2 {label}: start cannot silently replace an unconfirmed stop", code != 0 and "STOP_UNCONFIRMED" in message and self.journal(made["id"])["pendingAction"]["action"] == "stop", message)
        self.provider_up()
        if page:
            view = self.page(made["id"], "Retry stop")
            self.check("R2 page: the window offers Retry stop and reaches stopped", view.get("clicked") and "stopped" in (view.get("after", {}).get("badge") or ""), view)
        else:
            code, journal, message = self.manage("apply", "--setup", made["id"])
            self.check("R2 cli: apply honors the held stop", code == 0 and journal["state"] == "stopped", f"{message} {journal and journal.get('lastRefusal')}")
        final = self.journal(made["id"])
        self.check(f"R2 {label}: the action cleared, and retry started nothing", final.get("pendingAction") is None and len(self.creates(made["channel"])) == creates_before, final.get("pendingAction"))

    def r2_killed_stop(self):
        made = self.started("R2 killed")
        killed = self.kill_when(made["id"], self.at_step("stop"), verb="stop")
        self.check("R2 killed: stop was killed mid-way", killed, "")
        self.check("R2 killed: reads interrupted", self.state(made["id"]) == "interrupted", self.state(made["id"]))
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("R2 killed: retry finishes the stop, with its own id", code == 0 and journal["state"] == "stopped", f"{message} {journal and journal.get('lastRefusal')}")

    # ── R3: cancel tells the truth about a live execution ──
    def r3_cancel_live(self, page):
        label = "page" if page else "cli"
        made = self.fresh(f"R3 {label}")
        killed = self.kill_when(made["id"], self.at_step("verify"))
        self.check(f"R3 {label}: killed after the start landed", killed, "")
        code, cancelled, message = self.manage("cancel", "--setup", made["id"])
        residual = (cancelled or {}).get("residual", {})
        self.check(f"R3 {label}: cancel names the execution still held", code == 0 and residual.get("executionsHeld"), residual)
        self.check(f"R3 {label}: its next step says to stop it", "still running" in residual.get("next", ""), residual.get("next"))
        self.check(f"R3 {label}: the state is not plain cancelled", self.state(made["id"]) == "cancelled_held", self.state(made["id"]))
        actor = self.journal(made["id"]).get("agentPubkey") or residual.get("identityReserved")
        request = self.mention(actor, made["channel"], "@R3 are you there")
        self.answer_to(actor, made["channel"], request)
        self.check(f"R3 {label}: it does answer, which is why that state exists", True)
        if page:
            view = self.page(made["id"], "Stop")
            before = view.get("before", {})
            self.check("R3 page: the badge says it is still running", "still running" in (before.get("badge") or ""), before)
            self.check("R3 page: Stop is offered and stops it", view.get("clicked") and "stopped" in (view.get("after", {}).get("badge") or ""), view)
        else:
            code, journal, message = self.manage("stop", "--setup", made["id"])
            self.check("R3 cli: stop works on it", code == 0 and journal["state"] == "stopped", message)
            self.check("R3 cli: after the stop the record reads stopped", self.state(made["id"]) == "stopped", self.state(made["id"]))

    # ── R4: running means an open record ──
    def r4_external_stop(self, page):
        label = "page" if page else "cli"
        made = self.started(f"R4 {label}")
        row = next(r for r in self.status_sessions() if r.get("actor") == made["actor"])
        stopped = self.raw("sessions", "stop", "--channel", made["channel"], "--session", row["sessionId"], "--provider-authority", self.provider, "--wait", key=self.owner_key)
        self.check(f"R4 {label}: the execution is stopped by another path", stopped.returncode == 0, stopped.stderr[-300:])
        self.wait_for("record closed", lambda: self.closed_flag(row["sessionId"]) is True, 20)
        self.check(f"R4 {label}: the provider's record is closed:true", True)
        self.check(f"R4 {label}: running does not stay on the record", self.state(made["id"]) == "not_held", self.state(made["id"]))
        code, inspected, _ = self.manage("inspect", "--setup", made["id"])
        self.check(f"R4 {label}: inspect names the newest execution word", code == 0 and inspected["live"].get("execution") not in (None, "live"), inspected["live"].get("execution"))
        if page:
            view = self.page(made["id"], "Start")
            self.check("R4 page: the badge says not running, and Start returns it", "not running" in (view.get("before", {}).get("badge") or "") and (view.get("after", {}).get("badge") or "") == "running", view)
        else:
            code, journal, message = self.manage("start", "--setup", made["id"])
            self.check("R4 cli: start returns the same identity", code == 0 and journal["state"] == "running" and journal["agentPubkey"] == made["actor"], f"{message}")
        self.check(f"R4 {label}: an open record reads running again", self.state(made["id"]) == "running", self.state(made["id"]))

    def status_sessions(self):
        return json.loads(self.raw("host", "status").stdout)["sessions"]["sessions"]

    def closed_flag(self, session_id):
        return json.loads((self.state_dir() / "state.json").read_text())["sessions"][session_id].get("closed")

    # ── R5: a label cannot fall through a failed discovery ──
    def r5_failed_discovery(self):
        slug = f"r{uuid.uuid4().hex[:8]}"
        channel = self.channel(slug)
        project, code_dir, agents, model = self.project(slug, channel)
        self.manage("trust", "--project", project, "--checkout", code_dir, "--agents-repo", agents)
        host_json = self.home / ".local/state/buzz/host/host.json"
        original = host_json.read_text()
        edited = json.loads(original)
        edited["runtimes"][0]["discoverModels"] = True
        host_json.write_text(json.dumps(edited))
        try:
            self.raw("host", "bind")
            row = self.wait_for("discovery failure", lambda: next((r for r in [self.inventory()["runtimes"][0]] if r["readiness"] == "discovery_failed"), None), 90)
            self.check("R5: the label is not displayed as on offer", row["selectableModels"] == [], row["selectableModels"])
            self.check("R5: the row says why", "default" in row["readinessDetail"] and "did not answer" in row["readinessDetail"], row["readinessDetail"])
            before = len(self.definitions("R5 agent"))
            code, report, _ = self.manage("preflight", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "R5 agent", model))
            refused = [c for c in (report or {}).get("checks", []) if not c["ok"]]
            self.check("R5: preflight refuses by name", code == 0 and report["ok"] is False and any(c["code"] == "MODEL_UNCONFIRMED_BY_ADAPTER" for c in refused), refused)
            code, _, message = self.manage("plan", "--spec-file", "-", stdin=self.spec(project, channel, "lead", "R5 agent", model))
            self.check("R5: plan refuses before any effect", code != 0 and "MODEL_UNCONFIRMED_BY_ADAPTER" in message and len(self.definitions("R5 agent")) == before, message)
        finally:
            host_json.write_text(original)
            self.raw("host", "bind")
            self.wait_for("catalog restored", lambda: self.inventory()["runtimes"][0]["readiness"] == "unconfirmed", 60)

    # ── R6: a reservation survives the source moving ──
    def r6_source_moved(self, page):
        label = "page" if page else "cli"
        made = self.fresh(f"R6 {label}")
        journal = self.journal(made["id"])
        operation = journal["installOperation"]
        killed = self.kill_when(made["id"], lambda _: self.operation_file(operation).exists())
        after_kill = self.journal(made["id"])
        self.check(f"R6 {label}: killed with the install journaled and no identity recorded", killed and after_kill.get("agentPubkey") is None, f"{killed} {after_kill.get('agentPubkey')}")
        held = json.loads(self.operation_file(operation).read_text())
        pinned = held["request"]["privateConfig"]["record"]["role_material"]["pack_ref"]["sha"]
        reserved = held.get("actor_pubkey") or held["actorPubkey"]
        (made["agents"] / "moved.md").write_text("the source moved\n")
        self.git("add", ".", cwd=made["agents"])
        self.git("commit", "-s", "-m", "Move the source during a setup", cwd=made["agents"])
        self.git("push", "origin", "HEAD:main", cwd=made["agents"])
        moved = self.git("rev-parse", "HEAD", cwd=made["agents"])
        self.check(f"R6 {label}: the source really moved", moved != pinned, moved)
        if page:
            view = self.page(made["id"], "Retry")
            self.check("R6 page: Retry in the window reaches running", (view.get("after", {}).get("badge") or "") == "running", view)
            final = self.journal(made["id"])
        else:
            code, final, message = self.manage("apply", "--setup", made["id"])
            self.check("R6 cli: retry completes rather than a permanent conflict", code == 0 and final["state"] == "running", f"{message} {final and final.get('lastRefusal')}")
            final = final or {}
        self.check(f"R6 {label}: the reserved identity is the one that runs", final.get("agentPubkey") == reserved, final.get("agentPubkey"))
        again = json.loads(self.operation_file(operation).read_text())
        self.check(f"R6 {label}: the pin stays at the commit first resolved", again["request"]["privateConfig"]["record"]["role_material"]["pack_ref"]["sha"] == pinned, pinned)
        self.check(f"R6 {label}: one signed definition", len(self.definitions(f"R6 {label}")) == 1, "")
        return made, operation

    def r6_different_intent(self, made, operation):
        templates = base.ROOT / "personas/templates"
        result = self.raw("sessions", "hire-host", "install-role", "--operation-id", operation, "--home", self.home, "--project", made["project"], "--channel", made["channel"], "--role", "lead",
                          "--name", "A different name", "--provider-instance", "acme-primary", "--model", made["model"], "--parallelism", "1", "--templates", templates, key=self.owner_key)
        self.check("R6 control: the same operation with a different intent stays a conflict", result.returncode != 0 and "different" in (result.stderr + result.stdout).lower(), result.stderr[-400:])

    # ── M1: a start cannot skip the context step ──
    def m1_context(self):
        made = self.fresh("M1")
        killed = self.kill_when(made["id"], self.at_step("context"))
        self.check("M1: killed before the context was bound", killed, "")
        first = self.journal(made["id"])
        if first.get("agentPubkey") is None:
            self.manage("apply", "--setup", made["id"])
        code, journal, message = self.manage("start", "--setup", made["id"])
        self.check("M1: start leaves the lead with its project context", code == 0 and journal.get("projectContext") == "bound", f"{message} {journal and journal.get('projectContext')}")
        actor = journal["agentPubkey"]
        request = self.mention(actor, made["channel"], "@M1 which project")
        answer = self.answer_to(actor, made["channel"], request)
        self.check("M1: the turn after such a start knows its project", f"project-context={made['project']}" in answer[0]["content"], answer[0]["content"])

    # ── Races, and a cancel over an unknown outcome ──
    def races(self):
        path, away = self.credential_paths()
        made = self.fresh("Race refused")
        shutil.move(path, away)
        self.manage("apply", "--setup", made["id"])
        shutil.move(away, path)
        fresh = self.journal(made["id"])["pendingAction"]["operationId"]
        code, cancelled, message = self.manage("cancel", "--setup", made["id"])
        self.check("race: a refused setup can be cancelled, keeping its next id", code == 0 and self.journal(made["id"])["pendingAction"]["operationId"] == fresh, message)
        code, resumed, message = self.manage("apply", "--setup", made["id"], "--resume-cancelled")
        self.check("race: resuming after a signed refusal and a cancel reaches running", code == 0 and resumed["state"] == "running" and resumed["startOperation"] == fresh, f"{message} {resumed and resumed.get('lastRefusal')}")

        with (self.state_dir() / "agent-setups/apply.lock").open("a") as lock:
            import fcntl
            fcntl.flock(lock, fcntl.LOCK_EX)
            code, _, message = self.manage("cancel", "--setup", made["id"])
            self.check("race: cancel cannot cross a running action", code != 0 and "SETUP_BUSY" in message, message)
            fcntl.flock(lock, fcntl.LOCK_UN)

        self.manage("stop", "--setup", made["id"])
        creates = len(self.creates(made["channel"]))
        command = [str(base.BEE), "host", "manage", "start", "--setup", made["id"], "--owner-key-file", str(self.key_file)]
        both = [subprocess.Popen(command, env=self.env, cwd=self.work, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) for _ in range(2)]
        ends = [process.communicate(timeout=150) + (process.returncode,) for process in both]
        winners = [end for end in ends if end[2] == 0]
        losers = [end for end in ends if end[2] != 0]
        self.check("race: of two simultaneous starts one runs and one is told SETUP_BUSY", len(winners) == 1 and len(losers) == 1 and "SETUP_BUSY" in losers[0][1], [end[1][-200:] for end in ends])
        self.check("race: only one new start command was published", len(self.creates(made["channel"])) == creates + 1, "")

        self.manage("stop", "--setup", made["id"])
        self.provider_down()
        code, journal, message = self.manage("start", "--setup", made["id"], timeout=120)
        unknown = self.journal(made["id"])["pendingAction"]["operationId"]
        code, cancelled, message = self.manage("cancel", "--setup", made["id"])
        residual = (cancelled or {}).get("residual", {})
        self.check("cancel: over an unknown start, it says the outcome is unknown", residual.get("actionOutcomeUnknown") == "start" and "unknown" in residual.get("next", ""), residual)
        code, _, message = self.manage("stop", "--setup", made["id"])
        self.check("cancel: a stop cannot pass a start still on its way", code != 0 and "START_UNCONFIRMED" in message, message)
        self.provider_up()
        deadline = time.monotonic() + 150
        while True:
            code, journal, message = self.manage("apply", "--setup", made["id"], "--resume-cancelled")
            if code == 0 or time.monotonic() > deadline:
                break
            time.sleep(5)
        self.check("cancel: resume resolves the same id, without a new one", code == 0 and journal["startOperation"] == unknown, f"{message} {journal and journal.get('lastRefusal')}")

    # ── A window with a working control, whatever was held ──
    def b7_stop_held(self):
        made = self.started("B7 stop")
        self.check("B7 stop: stop killed mid-way", self.kill_when(made["id"], self.at_step("stop"), verb="stop"), "")
        view = self.page(made["id"], "Cancel")
        after = view.get("after", {})
        self.check("B7 stop: the sentence tells the truth about what is held", "Retry stop" in view.get("result", "") and "running" in view.get("result", ""), view.get("result"))
        self.check("B7 stop: the card offers the control that sentence names", "Retry stop" in after.get("buttons", []), after)
        view = self.page(made["id"], "Retry stop")
        self.check("B7 stop: clicking it from the window stops the agent", "stopped" in (view.get("after", {}).get("badge") or ""), view)

    def b7_start_held(self):
        made = self.started("B7 start")
        self.manage("stop", "--setup", made["id"])
        def open_and_held(journal):
            return (journal.get("pendingAction") or {}).get("action") == "start" and any(
                not session.get("closed") and session.get("actor") == made["actor"]
                for session in json.loads((self.state_dir() / "state.json").read_text())["sessions"].values())
        self.check("B7 start: killed once an execution was open", self.kill_when(made["id"], open_and_held, verb="start"), "")
        view = self.page(made["id"], "Cancel")
        buttons = view.get("after", {}).get("buttons", [])
        control = "Resume" if "Resume" in buttons else "Stop" if "Stop" in buttons else None
        self.check("B7 start: the card has a working control, not only Inspect", control is not None, buttons)
        if control:
            view = self.page(made["id"], control)
            after = view.get("after", {})
            self.check(f"B7 start: {control} acts, and no refusal says to use a button that is absent", not after.get("refusal") and (after.get("badge") in ("running", "stopped")), view)

    # ── A used start id is not reused for ever ──
    def b8_stop_cancel_resume(self):
        made = self.started("B8")
        landed = self.journal(made["id"])["startOperation"]
        view = self.page(made["id"], "Stop")
        self.check("B8: stopped from the window", "stopped" in (view.get("after", {}).get("badge") or ""), view)
        self.check("B8: Cancel means nothing on a stopped agent, so it is not offered", "Cancel" not in view.get("after", {}).get("buttons", []), view.get("after"))
        self.manage("cancel", "--setup", made["id"])
        view = self.page(made["id"], "Resume")
        self.check("B8: Resume from the window reaches running", view.get("after", {}).get("badge") == "running", view)
        final = self.journal(made["id"])
        self.check("B8: a new start id, recorded after the old one was closed", final["startOperation"] != landed, final["startOperation"])
        self.check("B8: one identity", len(self.definitions("B8")) == 1, "")
        self.check("B8: two created receipts, one per start", len(self.created_receipts(made["channel"])) == 2, "")

    # ── Words from what is known ──
    def b10_provider_down(self):
        made = self.started("B10")
        self.kill_when(made["id"], self.at_step("stop"), verb="stop")
        self.provider_down()
        code, cancelled, message = self.manage("cancel", "--setup", made["id"])
        next_step = (cancelled or {}).get("residual", {}).get("next", "")
        self.check("B10: cancel does not claim it is answering with no provider", code == 0 and "answering" not in next_step and "still running" not in next_step and "provider" in next_step, next_step)
        self.provider_up()
        code, journal, message = self.manage("apply", "--setup", made["id"], "--resume-cancelled")
        self.check("B10: resume finishes the held stop", code == 0 and journal["state"] == "stopped", f"{message} {journal and journal.get('lastRefusal')}")

    # ── A stop with nothing to stop ──
    def m9_nothing_started(self):
        made = self.fresh("M9")
        self.kill_when(made["id"], self.at_step("context"))
        code, journal, message = self.manage("stop", "--setup", made["id"])
        self.check("M9: stopping something never started is refused by name", code != 0 and "NOTHING_TO_STOP" in message, message)
        held = self.journal(made["id"]).get("pendingAction")
        self.check("M9: it leaves nothing held", held is None, held)
        code, _, message = self.manage("start", "--setup", made["id"])
        self.check("M9: it does not claim a stop was sent", "STOP_UNCONFIRMED" not in message, message)
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("M9: the setup still completes", journal is not None and journal["state"] == "running", f"{message} {journal and journal.get('lastRefusal')}")

    # ── Signed events the relay will no longer take ──
    def sign_aged(self, event, key):
        """Re-sign an event with an old timestamp: the relay's refusal is then real."""
        script = "import {finalizeEvent} from './_common.mjs'; const e=JSON.parse(process.argv[1]); const k=Uint8Array.from(Buffer.from(process.argv[2],'hex')); console.log(JSON.stringify(finalizeEvent({kind:e.kind,tags:e.tags,content:e.content,created_at:Math.floor(Date.now()/1000)-1000},k)));"
        return json.loads(base.out("node", "--input-type=module", "-e", script, json.dumps(event), key, env=dict(self.env), cwd=base.FOREIGN))

    def age_events(self, path, key_for, keep=lambda event: False):
        journal = json.loads(path.read_text())
        field = "event" if "event" in journal else "events"
        events = [journal[field]] if field == "event" else journal[field]
        aged = [event if keep(event) or event["pubkey"] not in key_for else self.sign_aged(event, key_for[event["pubkey"]]) for event in events]
        journal[field] = aged[0] if field == "event" else aged
        os.chmod(path, 0o600)
        path.write_text(json.dumps(journal))
        return aged

    def stored_ids(self, ids, channel=None):
        rows = self.read([44221, 44226, 30177, 0], f"stored-{uuid.uuid4().hex[:6]}.json", channel=None, extra={"ids": ids})
        return {row["id"] for row in rows}

    def killed_before_stored(self, name, verb, predicate, operation_id_of, tries=4):
        """Kill an action with its signed events journaled and not yet on the relay."""
        for _ in range(tries):
            made = name()
            if not self.kill_when(made["id"], predicate(made), verb=verb):
                continue
            path = self.operation_file(operation_id_of(made))
            if not path.exists():
                continue
            journal = json.loads(path.read_text())
            events = [journal["event"]] if "event" in journal else journal.get("events", [])
            commands = [e["id"] for e in events if e["kind"] in (44221, 44226)]
            if commands and not self.stored_ids(commands):
                return made, path
        raise RuntimeError("could not kill before the relay stored the command")

    def e1_install_aged(self):
        made = self.fresh("E1")
        operation = self.journal(made["id"])["installOperation"]
        self.check("E1: killed with the install journaled", self.kill_when(made["id"], lambda _: self.operation_file(operation).exists()), "")
        path = self.operation_file(operation)
        held = json.loads(path.read_text())
        actor = held.get("actor_pubkey") or held["actorPubkey"]
        self.require_unpublished(actor)
        key = (held.get("custody_handoff") or held.get("custodyHandoff") or {}).get("key") or (self.state_dir() / "actors" / actor / "key").read_text().strip()
        hex_key = decode_nsec(key)
        self.age_events(path, {self.owner: self.owner_key, actor: hex_key})
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("E1: an install that expired unsent finishes, with the reserved identity", code == 0 and journal["state"] == "running" and journal["agentPubkey"] == actor, f"{message} {journal and journal.get('lastRefusal')}")
        self.check("E1: one signed definition", len(self.definitions("E1")) == 1, "")
        final = json.loads(path.read_text())
        self.check("E1: the old and new events are both named", len(final.get("supersededEvents") or final.get("superseded_events") or []) >= 1, list(final.keys()))
        self.check("E1: the pin did not move", final["request"]["privateConfig"]["record"]["role_material"]["pack_ref"]["sha"] == held["request"]["privateConfig"]["record"]["role_material"]["pack_ref"]["sha"], "")

    def require_unpublished(self, actor):
        rows = self.read([30177], f"def-{uuid.uuid4().hex[:6]}.json", extra={"authors": [self.owner], "#d": [actor]})
        if rows:
            raise RuntimeError("the definition reached the relay before the kill")

    def e1b_install_partial(self):
        made = self.fresh("E1b")
        operation = self.journal(made["id"])["installOperation"]
        self.kill_when(made["id"], lambda _: self.operation_file(operation).exists())
        path = self.operation_file(operation)
        held = json.loads(path.read_text())
        actor = held.get("actor_pubkey") or held["actorPubkey"]
        self.require_unpublished(actor)
        definition = next(e for e in held["events"] if e["kind"] == 30177)
        self.publish_as(self.owner_key, definition, "partial-definition")
        self.age_events(path, {self.owner: self.owner_key}, keep=lambda event: event["id"] == definition["id"] or event["pubkey"] != self.owner)
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("E1b: partly stored, it continues rather than wedges", code == 0 and journal["state"] == "running" and journal["agentPubkey"] == actor, f"{message} {journal and journal.get('lastRefusal')}")
        defs = self.definitions("E1b")
        self.check("E1b: the stored definition is the one that stands", len(defs) == 1 and defs[0]["id"] == definition["id"], [d["id"] for d in defs])

    def e2_start_aged(self):
        def build():
            made = self.started("E2 " + uuid.uuid4().hex[:4])
            self.manage("stop", "--setup", made["id"])
            return made
        def at_start(_):
            return lambda journal: (journal.get("pendingAction") or {}).get("attempted") is True and (journal.get("pendingAction") or {}).get("action") == "start" and self.operation_file(journal["pendingAction"]["operationId"]).exists()
        def op_id(made):
            return self.journal(made["id"])["pendingAction"]["operationId"]
        made, path = self.killed_before_stored(build, "start", at_start, op_id)
        old = op_id(made)
        creates = len(self.creates(made["channel"]))
        self.age_events(path, {self.owner: self.owner_key})
        # First with the network unavailable: that proves nothing, so nothing moves.
        relay = self.processes[0]
        os.killpg(relay.pid, signal.SIGSTOP)
        try:
            env_before = dict(self.env)
            self.env.update({"BEEKEEPER_TIMEOUT_SECS": "2", "BEEKEEPER_CONNECT_TIMEOUT_SECS": "2"})
            code, journal, message = self.manage("apply", "--setup", made["id"], timeout=90)
        finally:
            self.env.clear()
            self.env.update(env_before)
            os.killpg(relay.pid, signal.SIGCONT)
        held = self.journal(made["id"]).get("pendingAction") or {}
        self.check("E2 unknown: with the relay unreachable the id does not change", code != 0 and held.get("operationId") == old and "JOURNAL_EVENT_EXPIRED" not in message, f"{message} {held}")
        self.check("E2 unknown: and it does not claim anything was refused", (self.journal(made["id"]).get("lastRefusal") or {}).get("uncertain") is True, self.journal(made["id"]).get("lastRefusal"))
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("E2: expired and unstored, retry reaches running on a new id", code == 0 and journal["state"] == "running" and journal["startOperation"] != old, f"{message} {journal and journal.get('lastRefusal')}")
        attempts = (journal or {}).get("attempts", [])
        self.check("E2: the closed attempt is recorded with its evidence", any(a["operationId"] == old and a["reason"] == "JOURNAL_EVENT_EXPIRED" and a["replacedBy"] == journal["startOperation"] for a in attempts), attempts)
        self.check("E2: exactly one new start command on the relay", len(self.creates(made["channel"])) == creates + 1, "")

    def e3_stop_aged(self):
        def build():
            return self.started("E3 " + uuid.uuid4().hex[:4])
        def at_stop(_):
            def predicate(journal):
                held = journal.get("pendingAction") or {}
                if held.get("action") != "stop":
                    return False
                path = self.operation_file(held["operationId"])
                try:
                    return bool(json.loads(path.read_text()).get("events"))
                except (OSError, ValueError):
                    return False
            return predicate
        def op_id(made):
            return self.journal(made["id"])["pendingAction"]["operationId"]
        made, path = self.killed_before_stored(build, "stop", at_stop, op_id)
        old = op_id(made)
        self.age_events(path, {self.owner: self.owner_key})
        view = self.page(made["id"], "Retry stop")
        self.check("E3: expired and unstored, Retry stop from the window stops it", "stopped" in (view.get("after", {}).get("badge") or ""), view)
        final = self.journal(made["id"])
        self.check("E3: the replaced attempt is recorded", any(a["operationId"] == old for a in final.get("attempts", [])), final.get("attempts"))

    def e4_stored_ack_lost(self):
        made = self.started("E4")
        self.manage("stop", "--setup", made["id"])
        creates = len(self.creates(made["channel"]))
        self.provider_down()
        process = subprocess.Popen([str(base.BEE), "host", "manage", "start", "--setup", made["id"], "--owner-key-file", str(self.key_file)], env=self.env, cwd=self.work, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.wait_for("the command stored", lambda: len(self.creates(made["channel"])) == creates + 1, 45)
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()
        old = self.journal(made["id"])["pendingAction"]["operationId"]
        path = self.operation_file(old)
        stored = [e["id"] for e in json.loads(path.read_text())["events"] if e["kind"] == 44221]
        self.check("E4: the command was accepted and its reply lost", bool(self.stored_ids(stored)), stored)
        self.age_events(path, {self.owner: self.owner_key}, keep=lambda event: event["kind"] == 44221 or bool(self.stored_ids([event["id"]])))
        # With the provider still away, the only honest answer is unknown: its stale
        # membership commands must not turn an accepted start into a refusal.
        code, journal, message = self.manage("start", "--setup", made["id"], timeout=120)
        last = (self.journal(made["id"]).get("lastRefusal") or {})
        self.check("E4 away: an accepted command with its reply lost reads unknown, not refused", code != 0 and last.get("uncertain") is True and "too far" not in message and "EXPIRED" not in message, f"{message} {last}")
        self.check("E4 away: the same id is held", self.journal(made["id"])["pendingAction"]["operationId"] == old, "")
        self.check("E4 away: no second command", len(self.creates(made["channel"])) == creates + 1, "")
        self.provider_up()
        deadline = time.monotonic() + 120
        said = []
        while True:
            code, journal, message = self.manage("apply", "--setup", made["id"])
            said.append(message)
            if code == 0 or time.monotonic() > deadline:
                break
            time.sleep(5)
        expired = [m for m in said if "JOURNAL_EVENT_EXPIRED" in m or "too far" in m]
        self.check("E4: no retry calls an accepted command expired", not expired, expired[:1])
        self.check("E4: no second command", len(self.creates(made["channel"])) == creates + 1, len(self.creates(made["channel"])))
        self.check("E4: the same id reaches running", code == 0 and journal["state"] == "running" and journal["startOperation"] == old, f"{message} {journal and journal.get('lastRefusal')}")
        self.transitions(made, "E4", page=False)

    def e5_context_aged(self):
        def build():
            return self.fresh("E5 " + uuid.uuid4().hex[:4])
        def at_context(_):
            return lambda journal: journal["steps"] and journal["steps"][-1]["step"] == "context" and self.operation_file(journal["contextOperation"]).exists()
        def op_id(made):
            return self.journal(made["id"])["contextOperation"]
        made, path = self.killed_before_stored(build, "apply", at_context, op_id)
        old = op_id(made)
        self.age_events(path, {self.owner: self.owner_key})
        code, journal, message = self.manage("apply", "--setup", made["id"])
        self.check("E5: an expired founding is replaced, and the lead is bound", code == 0 and journal["state"] == "running" and journal["projectContext"] == "bound" and journal["contextOperation"] != old, f"{message} {journal and journal.get('lastRefusal')}")
        actor = journal["agentPubkey"] if journal else None
        if actor:
            answer = self.answer_to(actor, made["channel"], self.mention(actor, made["channel"], "@E5 which project"))
            self.check("E5: the turn knows its project", f"project-context={made['project']}" in answer[0]["content"], answer[0]["content"])

    def real_expiry(self):
        """The one test that waits for real: three unsent actions, past the relay's window."""
        def stopped():
            made = self.started("Real start " + uuid.uuid4().hex[:4])
            self.manage("stop", "--setup", made["id"])
            return made
        pending_id = lambda made: self.journal(made["id"])["pendingAction"]["operationId"]
        start, _ = self.killed_before_stored(stopped, "start", lambda _: lambda j: (j.get("pendingAction") or {}).get("attempted") is True and self.operation_file(j["pendingAction"]["operationId"]).exists(), pending_id)
        def stop_ready(_):
            def predicate(j):
                held = j.get("pendingAction") or {}
                try:
                    return held.get("action") == "stop" and bool(json.loads(self.operation_file(held["operationId"]).read_text()).get("events"))
                except (OSError, ValueError):
                    return False
            return predicate
        stop, _ = self.killed_before_stored(lambda: self.started("Real stop " + uuid.uuid4().hex[:4]), "stop", stop_ready, pending_id)
        install = self.fresh("Real install")
        operation = self.journal(install["id"])["installOperation"]
        self.kill_when(install["id"], lambda _: self.operation_file(operation).exists())
        started_at = time.monotonic()
        self.save("real-expiry-before.json", {"start": self.journal(start["id"]), "stop": self.journal(stop["id"]), "install": self.journal(install["id"])})
        while time.monotonic() - started_at < 960:
            self.inventory_or_none()
            time.sleep(30)
        code, journal, message = self.manage("apply", "--setup", start["id"])
        self.check("real wait, 960 s: a start that expired unsent reaches running on a new id", code == 0 and journal["state"] == "running", f"{message} {journal and journal.get('lastRefusal')}")
        code, journal, message = self.manage("apply", "--setup", stop["id"])
        self.check("real wait, 960 s: a stop that expired unsent stops", code == 0 and journal["state"] == "stopped", f"{message} {journal and journal.get('lastRefusal')}")
        code, journal, message = self.manage("apply", "--setup", install["id"])
        self.check("real wait, 960 s: an install that expired unsent finishes", code == 0 and journal["state"] == "running", f"{message} {journal and journal.get('lastRefusal')}")
        self.check("real wait, 960 s: one definition for that install", len(self.definitions("Real install")) == 1, "")
        self.save("real-expiry-after.json", {"start": self.journal(start["id"]), "stop": self.journal(stop["id"]), "install": self.journal(install["id"])})

    # ── Nothing recovered may be left that cannot be stopped ──
    def transitions(self, made, label, page):
        """Stop, then Start, then read state: what B11 found impossible after a Retry."""
        actor = self.journal(made["id"])["agentPubkey"]
        before = len(self.creates(made["channel"]))
        if page:
            view = self.page(made["id"], "Stop")
            stopped = "stopped" in (view.get("after", {}).get("badge") or "")
            detail = view
        else:
            code, journal, message = self.manage("stop", "--setup", made["id"])
            stopped = code == 0 and journal["state"] == "stopped"
            detail = f"{message} {journal and journal.get('lastRefusal')}"
        self.check(f"{label}: after recovery, Stop stops it", stopped, detail)
        open_now = [s for s in json.loads((self.state_dir() / "state.json").read_text())["sessions"].values() if not s.get("closed") and s.get("actor") == actor]
        self.check(f"{label}: and the provider's record agrees", not open_now, open_now)
        if page:
            view = self.page(made["id"], "Start")
            started = view.get("after", {}).get("badge") == "running"
            detail = view
        else:
            code, journal, message = self.manage("start", "--setup", made["id"])
            started = code == 0 and journal["state"] == "running"
            detail = f"{message} {journal and journal.get('lastRefusal')}"
        self.check(f"{label}: and Start runs the same identity again", started and self.journal(made["id"])["agentPubkey"] == actor, detail)
        self.check(f"{label}: with exactly one new start command", len(self.creates(made["channel"])) == before + 1, len(self.creates(made["channel"])) - before)

    def killed_first_start(self, name):
        """A first start killed with its execution open and no receipt recorded."""
        def predicate(journal):
            actor = journal.get("agentPubkey")
            if not actor or not journal["steps"] or journal["steps"][-1]["step"] != "start":
                return False
            try:
                sessions = json.loads((self.state_dir() / "state.json").read_text())["sessions"].values()
            except (OSError, ValueError):
                return False
            return any(not s.get("closed") and s.get("actor") == actor for s in sessions)
        for attempt in range(4):
            made = self.fresh(f"{name} {uuid.uuid4().hex[:4]}")
            if self.kill_when(made["id"], predicate):
                self.check(f"{name}: killed with its execution open and no action held", self.journal(made["id"]).get("pendingAction") is None, self.journal(made["id"]).get("pendingAction"))
                return made
        raise RuntimeError("could not kill a first start with its execution open")

    def q1_first_start_cancel(self):
        made = self.killed_first_start("Q1")
        view = self.page(made["id"], "Cancel")
        buttons = view.get("after", {}).get("buttons", [])
        self.check("Q1: after Cancel the card offers Resume", "Resume" in buttons, view.get("after"))
        view = self.page(made["id"], "Resume")
        self.check("Q1: Resume from the window reaches running", view.get("after", {}).get("badge") == "running", view)
        self.transitions(made, "Q1", page=True)

    def q3_retry_then_stop(self, wait, page):
        label = f"Q3{'a' if wait else 'c'} {'page' if page else 'cli'}"
        made = self.killed_first_start(label)
        if wait:
            time.sleep(wait)
        creates = len(self.creates(made["channel"]))
        if page:
            view = self.page(made["id"], "Retry")
            reached = view.get("after", {}).get("badge") == "running"
            detail = view
        else:
            code, journal, message = self.manage("apply", "--setup", made["id"])
            reached = code == 0 and journal["state"] == "running"
            detail = f"{message} {journal and journal.get('lastRefusal')}"
        self.check(f"{label}: Retry reaches running", reached, detail)
        self.check(f"{label}: Retry started no new execution", len(self.creates(made["channel"])) == creates, "")
        self.transitions(made, label, page)

    def q3b_lost_receipt(self, page):
        label = f"Q3b {'page' if page else 'cli'}"
        made = self.started(label)
        self.manage("stop", "--setup", made["id"])
        confirmed_before = self.journal(made["id"]).get("confirmedStart")
        self.provider_down()
        code, journal, message = self.manage("start", "--setup", made["id"], timeout=120)
        unknown = self.journal(made["id"])["pendingAction"]["operationId"]
        self.check(f"{label}: an unanswered start reads unknown", code != 0 and (journal or {}).get("lastRefusal", {}).get("uncertain") is True, message)
        if confirmed_before:
            self.check(f"{label}: an earlier confirmation does not mark this attempt landed", confirmed_before["operationId"] != unknown, unknown)
        code, _, message = self.manage("stop", "--setup", made["id"])
        self.check(f"{label}: so Stop is still refused by name", code != 0 and "START_UNCONFIRMED" in message, message)
        self.provider_up()
        deadline = time.monotonic() + 150
        creates = len(self.creates(made["channel"]))
        while True:
            if page:
                view = self.page(made["id"], "Retry")
                reached = view.get("after", {}).get("badge") == "running"
            else:
                code, journal, message = self.manage("apply", "--setup", made["id"])
                reached = code == 0
            if reached or time.monotonic() > deadline:
                break
            time.sleep(5)
        self.check(f"{label}: Retry resolves it on the same id", reached and self.journal(made["id"])["startOperation"] == unknown, self.journal(made["id"])["startOperation"])
        self.check(f"{label}: without another command", len(self.creates(made["channel"])) == creates, "")
        self.transitions(made, label, page)

    def q4_restart(self, made, label):
        """A Host restart over a start whose outcome this setup has or has not recorded.

        The scripted runtime refuses `session/load`, so no lease returns after a restart
        and `running` cannot be verified: the setup must say so, start nothing new, and
        still be able to stop and start. A real adapter's restore is not covered here.
        """
        self.stop_host()
        self.start_host()
        self.wait_for("catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)
        before = len(self.creates(made["channel"]))
        code, journal, message = self.manage("apply", "--setup", made["id"])
        state = (journal or {}).get("state")
        honest = state == "running" or (state == "needs_attention" and journal["lastRefusal"]["uncertain"] is True)
        self.check(f"{label}: after a Host restart, Retry reports what it can verify", honest, f"{state} {message}")
        self.check(f"{label}: and starts no new execution", len(self.creates(made["channel"])) == before, len(self.creates(made["channel"])) - before)
        confirmed = self.journal(made["id"]).get("confirmedStart")
        self.check(f"{label}: the start it recovered is recorded as confirmed and current", confirmed and confirmed["operationId"] == self.journal(made["id"])["startOperation"] and self.journal(made["id"]).get("pendingAction") is None, confirmed)
        self.transitions(made, label, page=False)

    def q4_restart_before_promotion(self):
        self.q4_restart(self.killed_first_start("Q4 start"), "Q4 start")
        made = self.fresh("Q4 verify")
        self.check("Q4 verify: killed once the start was confirmed", self.kill_when(made["id"], self.at_step("verify")), "")
        journal = self.journal(made["id"])
        self.check("Q4 verify: confirmation and promotion landed in one write", journal.get("confirmedStart") and journal["confirmedStart"]["operationId"] == journal["startOperation"] and journal.get("pendingAction") is None, journal.get("confirmedStart"))
        self.q4_restart(made, "Q4 verify")

    # ── The attestation pointer, inherited from somewhere it does not belong ──
    def auth_pointer(self):
        sentinel = self.work / "inherited-tag"
        sentinel.write_text('["auth","POINTER_CONTENT_SENTINEL","",""]')
        sentinel.chmod(0o600)
        self.secrets.append("POINTER_CONTENT_SENTINEL")
        pointer = {"BEEKEEPER_AUTH_TAG_FILE": str(sentinel), "BUZZ_AUTH_TAG_FILE": str(sentinel)}

        saved = dict(self.env)
        self.env.update(pointer)
        try:
            result = self.raw("host", "manage", "channels", key=self.owner_key)
            self.check("pointer: an env key beside a pointer is refused as an agent's environment", result.returncode != 0 and "DELEGATED_SIGNER_REFUSED" in result.stderr, result.stderr[-300:])
            self.check("pointer: the refusal does not quote the file", "POINTER_CONTENT_SENTINEL" not in result.stderr + result.stdout, "")
            code, signer, message = self.manage("signer")
            self.check("pointer: a key file a person named still works, signing as the owner", code == 0 and signer["matches"] is True, message)
            code, channels, message = self.manage("channels")
            self.check("pointer: and is not attested by that file", code == 0 and isinstance(channels, list), message)
        finally:
            self.env.clear()
            self.env.update(saved)
        env = dict(self.env, BEEKEEPER_BEE=str(base.BEE), BEEKEEPER_PRIVATE_KEY=self.owner_key, BUZZ_PRIVATE_KEY=self.owner_key, **pointer)
        menu = subprocess.run([str(base.MENUBAR), "--manage-action", json.dumps({"kind": "signer"})], env=env, cwd=self.work, capture_output=True, text=True, timeout=60)
        outcome = json.loads(menu.stdout.strip().splitlines()[-1])
        self.check("pointer: the menu bar passes bee neither key nor pointer", outcome["ok"] is False and outcome["code"] == "OWNER_KEY_UNAVAILABLE", outcome)
        menu = subprocess.run([str(base.MENUBAR), "--manage-action", json.dumps({"kind": "signer", "keyFile": str(self.key_file)})], env=env, cwd=self.work, capture_output=True, text=True, timeout=60)
        outcome = json.loads(menu.stdout.strip().splitlines()[-1])
        self.check("pointer: and the window's key path still works through it", outcome["ok"] is True and outcome["value"]["matches"] is True, outcome)

        made = self.started("Pointer")
        self.stop_host()
        self.start_host(extra_env=pointer)
        self.wait_for("catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)
        self.manage("stop", "--setup", made["id"])
        code, journal, message = self.manage("start", "--setup", made["id"])
        self.check("pointer: a Host that inherits one still runs its agent", code == 0 and journal["state"] == "running", f"{message} {journal and journal.get('lastRefusal')}")
        answer = self.answer_to(made["actor"], made["channel"], self.mention(made["actor"], made["channel"], "@Pointer are you there"))
        self.check("pointer: and that agent answers, so the provider did not read it", len(answer) == 1, answer)
        logs = (self.work / "host.log").read_text(errors="replace")
        self.check("pointer: its contents reached no Host log", "POINTER_CONTENT_SENTINEL" not in logs, "")
        self.stop_host()
        self.start_host()
        self.wait_for("catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)

    # ── Restart ──
    def restart(self):
        made = self.started("Restart")
        self.stop_host()
        self.start_host()
        self.wait_for("catalog", lambda: self.inventory()["runtimes"][0]["selectableModels"], 30)
        row = self.setup_row(made["id"])
        self.check("restart: the Host reports the journal against the provider's record", row["effectiveState"] in ("running", "not_held") and row["setup"]["agentPubkey"] == made["actor"], row["effectiveState"])
        code, journal, message = self.manage("stop", "--setup", made["id"])
        self.check("restart: stop still works after the Host restart", code == 0 and journal["state"] == "stopped", f"{message} {journal and journal.get('lastRefusal')}")
        code, journal, message = self.manage("start", "--setup", made["id"])
        self.check("restart: start after a Host restart reaches running with one identity", code == 0 and journal["state"] == "running" and len(self.definitions("Restart")) == 1, f"{message} {journal and journal.get('lastRefusal')}")

    def scan(self):
        roots = [self.state_dir() / "agent-setups", self.work / "evidence", self.work / "manage-output.log"]
        leaks = []
        for root in roots:
            for path in root.rglob("*") if root.is_dir() else [root]:
                if path.is_file():
                    text = path.read_text(errors="replace")
                    if "nsec1" in text or any(secret in text for secret in self.secrets):
                        leaks.append(str(path))
        self.check("secrets: no key in journals, page reads or output", not leaks, leaks)


SCENARIOS = {
    "R1": lambda h: (h.r1_refusal_cli(), h.r1_refusal_page(), h.r1_unknown_keeps_id()),
    "R2": lambda h: (h.r2_stop_retry(False), h.r2_stop_retry(True), h.r2_killed_stop()),
    "R3": lambda h: (h.r3_cancel_live(False), h.r3_cancel_live(True)),
    "R4": lambda h: (h.r4_external_stop(False), h.r4_external_stop(True)),
    "R5": lambda h: h.r5_failed_discovery(),
    "R6": lambda h: (lambda result: (h.r6_different_intent(*result), h.r6_source_moved(True)))(h.r6_source_moved(False)),
    "M1": lambda h: h.m1_context(),
    "races": lambda h: h.races(),
    "restart": lambda h: h.restart(),
    "pointer": lambda h: h.auth_pointer(),
    "Q1": lambda h: h.q1_first_start_cancel(),
    "Q3": lambda h: (h.q3_retry_then_stop(8, False), h.q3_retry_then_stop(0, False), h.q3_retry_then_stop(8, True), h.q3_retry_then_stop(0, True)),
    "Q3b": lambda h: (h.q3b_lost_receipt(False), h.q3b_lost_receipt(True)),
    "Q4": lambda h: h.q4_restart_before_promotion(),
    "B7": lambda h: (h.b7_stop_held(), h.b7_start_held()),
    "B8": lambda h: h.b8_stop_cancel_resume(),
    "B10": lambda h: h.b10_provider_down(),
    "M9": lambda h: h.m9_nothing_started(),
    "E1": lambda h: h.e1_install_aged(),
    "E1b": lambda h: h.e1b_install_partial(),
    "E2": lambda h: h.e2_start_aged(),
    "E3": lambda h: h.e3_stop_aged(),
    "E4": lambda h: h.e4_stored_ack_lost(),
    "E5": lambda h: h.e5_context_aged(),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only")
    parser.add_argument("--real-expiry", action="store_true", help="also wait out the relay's window for real, about 16 minutes")
    parser.add_argument("--evidence")
    args = parser.parse_args()
    evidence = pathlib.Path(args.evidence) if args.evidence else None
    if evidence:
        evidence.mkdir(parents=True, exist_ok=True)
    harness = Recovery(evidence)
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError("overall limit")))
    signal.alarm(OVERALL_LIMIT_SECS)
    print(f"work: {harness.work}", flush=True)
    names = args.only.split(",") if args.only else list(SCENARIOS)
    if args.real_expiry:
        names.append("real-expiry")
    # Sixteen minutes of real waiting: asked for by name.
    extra = {"real-expiry": lambda h: h.real_expiry()}
    try:
        harness.start_services()
        harness.owner_setup()
        harness.start_host()
        for name in names:
            # One scenario's failure must not hide the others: that is the point
            # of running these against the unfixed tree as well.
            try:
                {**SCENARIOS, **extra}[name](harness)
            except Exception:
                harness.check(f"{name}: the scenario ran to its end", False, traceback.format_exc()[-900:])
                if harness.inventory_or_none() is None or harness.inventory()["host"]["provider"]["state"] != "live":
                    subprocess.run(["true"])
                    try:
                        harness.provider_up()
                    except Exception:
                        pass
            path, away = harness.credential_paths()
            if away.exists():
                shutil.move(away, path)
        harness.scan()
    except BaseException:
        harness.check("the run completed", False, traceback.format_exc()[-800:])
    finally:
        signal.alarm(0)
        harness.teardown()
    failed = [row for row in harness.results if not row["ok"]]
    summary = {"passed": len(harness.results) - len(failed), "failed": len(failed), "checks": harness.results}
    (harness.work / "results.json").write_text(json.dumps(summary, indent=2))
    if evidence:
        shutil.copy(harness.work / "results.json", evidence / "results.json")
        source = harness.work / "evidence"
        if source.exists():
            shutil.copytree(source, evidence / "records", dirs_exist_ok=True)
    print(f"{summary['passed']} passed, {summary['failed']} failed; retained {harness.work}", flush=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
