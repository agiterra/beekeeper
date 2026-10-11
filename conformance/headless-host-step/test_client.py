"""Offline contract tests. Keys below are public disposable test identities."""

import hashlib
import json
from pathlib import Path
import unittest

import client


def signed(secret, kind, tags, payload, timestamp=1791720000):
    point = client.mul(secret)
    scalar = secret if point[1] % 2 == 0 else client.N - secret
    public = point[0].to_bytes(32, "big")
    event = {"pubkey": public.hex(), "created_at": timestamp, "kind": kind,
             "tags": tags, "content": json.dumps(payload, separators=(",", ":"))}
    message = hashlib.sha256(json.dumps([0, event["pubkey"], timestamp, kind, tags,
                                       event["content"]], separators=(",", ":")).encode()).digest()
    # BIP-340 signing is fixture generation only: deterministic zero aux data.
    mask = client.tagged_hash("BIP0340/aux", bytes(32))
    key = bytes(a ^ b for a, b in zip(scalar.to_bytes(32, "big"), mask))
    nonce = int.from_bytes(client.tagged_hash("BIP0340/nonce", key + public + message), "big") % client.N
    r = client.mul(nonce)
    nonce = nonce if r[1] % 2 == 0 else client.N - nonce
    r_bytes = r[0].to_bytes(32, "big")
    e = int.from_bytes(client.tagged_hash("BIP0340/challenge", r_bytes + public + message), "big") % client.N
    event.update(id=message.hex(), sig=(r_bytes + ((nonce + e * scalar) % client.N).to_bytes(32, "big")).hex())
    return event


def fixture():
    run = "11111111-1111-4111-8111-111111111111"
    channel = "22222222-2222-4222-8222-222222222222"
    base = [["d", run + ":hook"], ["h", channel], ["schema", client.SCHEMA]]
    project = "30621:" + "a" * 64 + ":demo"
    request = signed(3, 46013, base + [["a", project]], {
        "schema": client.SCHEMA, "runId": run,
        "workflowId": "33333333-3333-4333-8333-333333333333",
        "workflowName": "on-push", "stepId": "hook", "stepIndex": 0,
        "definitionHash": "b" * 64, "stepKind": "run_on_host", "channelId": channel,
        "project": project, "triggerContext": {"after": "c" * 40},
        "inputs": {}, "expiresAt": 1791723600})
    fields = {"schema": client.SCHEMA, "runId": run, "stepId": "hook",
              "channelId": channel, "requestedEventId": request["id"]}
    claim = signed(4, 46022, base + [["e", request["id"]]], dict(fields, host={"name": "fixture"}))
    result = signed(4, 46023, base + [["e", request["id"]]],
                    dict(fields, claimEventId=claim["id"], disposition="exited", exitCode=0,
                         timedOut=False, agentsCommit="d" * 40, headSha="c" * 40,
                         stdoutTail='{"schema":"example-hook/v1","ok":true}', stderrTail=""))
    return {"witnessedRelayPubkey": request["pubkey"], "acceptedClaimEventId": claim["id"],
            "request": request, "claim": claim, "result": result}


class ExchangeTests(unittest.TestCase):
    def setUp(self):
        self.record = fixture()

    def resign(self, name, change, secret=None):
        event = self.record[name]
        payload = json.loads(event["content"])
        change(payload)
        self.record[name] = signed(secret or (3 if name == "request" else 4),
                                   event["kind"], event["tags"], payload, event["created_at"])

    def test_valid_and_committed_vector(self):
        self.assertEqual(client.exchange(self.record)["disposition"], "exited")
        with open(Path(__file__).with_name("exchange.json")) as source:
            self.assertEqual(json.load(source), self.record)

    def test_invalid_signature(self):
        self.record["request"]["sig"] = "0" * 128
        with self.assertRaisesRegex(ValueError, "signature"):
            client.exchange(self.record)

    def test_unwitnessed_relay(self):
        self.record["witnessedRelayPubkey"] = self.record["claim"]["pubkey"]
        with self.assertRaisesRegex(ValueError, "witnessed relay"):
            client.exchange(self.record)

    def test_changed_content_without_signature(self):
        self.record["result"]["content"] += " "
        with self.assertRaisesRegex(ValueError, "id mismatch"):
            client.exchange(self.record)

    def test_unknown_field(self):
        self.resign("result", lambda p: p.update(secret="no"))
        with self.assertRaisesRegex(ValueError, "unknown payload"):
            client.exchange(self.record)

    def test_request_cannot_carry_command(self):
        self.resign("request", lambda p: p.update(command=["sh", "-c", "bad"]))
        with self.assertRaisesRegex(ValueError, "unknown payload"):
            client.exchange(self.record)

    def test_wrong_request(self):
        self.resign("result", lambda p: p.update(requestedEventId="e" * 64))
        with self.assertRaisesRegex(ValueError, "request correlation"):
            client.exchange(self.record)

    def test_wrong_channel(self):
        self.resign("result", lambda p: p.update(channelId="44444444-4444-4444-8444-444444444444"))
        with self.assertRaisesRegex(ValueError, "responsibility correlation"):
            client.exchange(self.record)

    def test_wrong_host(self):
        self.resign("result", lambda p: None, secret=5)
        with self.assertRaisesRegex(ValueError, "winning claim"):
            client.exchange(self.record)

    def test_unaccepted_claim(self):
        self.record["acceptedClaimEventId"] = "e" * 64
        with self.assertRaisesRegex(ValueError, "not recorded as admitted"):
            client.exchange(self.record)

    def test_wrong_claim(self):
        self.resign("result", lambda p: p.update(claimEventId="e" * 64))
        with self.assertRaisesRegex(ValueError, "winning claim"):
            client.exchange(self.record)

    def test_duplicate_tag(self):
        event = self.record["result"]
        self.record["result"] = signed(4, event["kind"], event["tags"] + [event["tags"][0]],
                                        json.loads(event["content"]))
        with self.assertRaisesRegex(ValueError, "tags disagree"):
            client.exchange(self.record)

    def test_expired_claim(self):
        event = self.record["claim"]
        self.record["claim"] = signed(4, event["kind"], event["tags"],
                                       json.loads(event["content"]), 1791723600)
        with self.assertRaisesRegex(ValueError, "expiry"):
            client.exchange(self.record)

    def test_lost_effect_is_unknown(self):
        def lost(payload):
            payload.pop("exitCode")
            payload["disposition"] = "lost_on_restart"
        self.resign("result", lost)
        self.assertEqual(client.exchange(self.record)["disposition"], "lost_on_restart")

    def test_lost_cannot_assert_success(self):
        self.resign("result", lambda p: p.update(disposition="lost_on_restart"))
        with self.assertRaisesRegex(ValueError, "cannot assert an exit"):
            client.exchange(self.record)


if __name__ == "__main__":
    if "--generate" in __import__("sys").argv:
        Path(__file__).with_name("exchange.json").write_text(json.dumps(fixture(), indent=2) + "\n")
    else:
        unittest.main()
