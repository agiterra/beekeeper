/**
 * SV-91/SV-93 fixture: the 44225 envelopes of Brian's installed-build audit
 * session BK-AUDIT-1006 (becbd0cb, 2026-10-06), as the provider published
 * them — a subset of the 61, with private paths reduced to the redaction
 * marker and long tool output cut. Seq 31 announces background task
 * bi9cros3k; seq 35 is the `autonomous_turn_started` row the next turn opens
 * on; seq 52 the `autonomous_turn … task-notification` row that lands inside
 * its final answer. claude-agent-acp 0.84.0 sent no task-notification prompt.
 */
export const BK_AUDIT_1006_ENVELOPES = [
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 1,
    timestamp: 1791248021230,
    turnId: null,
    item: {
      kind: "status",
      status: "session_fresh",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 2,
    timestamp: 1791248021245,
    turnId: null,
    item: {
      kind: "status",
      reason: "full-access",
      status: "execution_boundary_not_enforced",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 3,
    timestamp: 1791248033714,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      commandId: "csc-94c5d946-f638-4c5d-891c-0a12b44c8127",
      content:
        "Audit run BK-AUDIT-1006. Work only inside this folder. Say one short line before each step:\n1. pwd && date -u; mkdir -p notes src && ls -la\n2. Create src/counter.py (count_words(path) + __main__) and notes/sample.txt (three lines).\n3. python3 src/counter.py notes/sample.txt\n4. python3 src/missing.py  (fails on purpose; say so in one line and carry on)\n5. Spawn one subagent to suggest one improvement to src/counter.py in one sentence; apply it.\n6. Run this in the background and end your turn: python3 -c \"import time; time.sleep(60); print('bg-done')\"\n   When it finishes, continue on your own with step 7.\n7. python3 -c \"import time; time.sleep(150); print('quiet-done')\"  (write nothing while it runs)\n8. Three separate commands, one line from you after each:\n   python3 -c \"import time; time.sleep(20); print('a')\"  (then 'b', then 'c')\n9. Summary: files, failed commands, start and end time (date -u).",
      kind: "user_prompt",
      operatorPubkey:
        "3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e",
      steered: false,
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 4,
    timestamp: 1791248036452,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "assistant_text",
      text: "Step 1: checking location and time, creating folders.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 5,
    timestamp: 1791248036564,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command: "pwd && date -u; mkdir -p notes src && ls -la",
        },
        toolId: "toolu_01JJR5uZ1fCHVavbwbGmGeMa",
        toolKind: "execute",
        toolName: "pwd && date -u; mkdir -p notes src && ls -la",
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 6,
    timestamp: 1791248037913,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      content:
        "```console\n[elided private context]\nTue Oct  6 00:53:57 UTC 2026\ntotal 16\ndrwxr-xr-x   6 brian  staff   192 Oct  5 20:53\n```",
      input: {
        command: "pwd && date -u; mkdir -p notes src && ls -la",
        description: "Show directory, time, create folders",
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_01JJR5uZ1fCHVavbwbGmGeMa",
      toolKind: "execute",
      toolName: "pwd && date -u; mkdir -p notes src && ls -la",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 12,
    timestamp: 1791248043341,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "assistant_text",
      text: "Step 3: running the counter on the sample.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 13,
    timestamp: 1791248043601,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command: "python3 src/counter.py notes/sample.txt",
        },
        toolId: "toolu_01GPnv3EwesSbGHrbESE3D6j",
        toolKind: "execute",
        toolName: "python3 src/counter.py notes/sample.txt",
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 14,
    timestamp: 1791248044271,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      content: "```console\n12\n```",
      input: {
        command: "python3 src/counter.py notes/sample.txt",
        description: "Count words in sample file",
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_01GPnv3EwesSbGHrbESE3D6j",
      toolKind: "execute",
      toolName: "python3 src/counter.py notes/sample.txt",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 29,
    timestamp: 1791248063351,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "assistant_text",
      text: "Still 12. Step 6: starting the 60-second job in the background and ending my turn.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 30,
    timestamp: 1791248063368,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command:
            "python3 -c \"import time; time.sleep(60); print('bg-done')\"",
        },
        toolId: "toolu_01G1xbH6HoXzyH5kKAoJq9RS",
        toolKind: "execute",
        toolName:
          "python3 -c \"import time; time.sleep(60); print('bg-done')\"",
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 31,
    timestamp: 1791248064721,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      content:
        "```console\nCommand running in background with ID: bi9cros3k. Output is being written to: /private/tmp/claude-502/tasks/bi9cros3k.output. You will be notified when it completes. To check interim output, use Read on that file path.\n```",
      input: {
        command: "python3 -c \"import time; time.sleep(60); print('bg-done')\"",
        description: "Run 60-second sleep job in background",
        run_in_background: true,
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_01G1xbH6HoXzyH5kKAoJq9RS",
      toolKind: "execute",
      toolName: "python3 -c \"import time; time.sleep(60); print('bg-done')\"",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 32,
    timestamp: 1791248066485,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "assistant_text",
      text: "The 60-second job is running in the background. I'll pick up at step 7 when it finishes.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 33,
    timestamp: 1791248066505,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      kind: "context_window_updated",
      usage: {
        size: 1000000,
        used: 31035,
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 34,
    timestamp: 1791248066616,
    turnId: "5be72eae-de29-4154-8c64-d91b91203116",
    item: {
      costBasis: "adapter_estimate",
      costUsd: 0.3958032,
      durationMs: 32961,
      inputTokens: 236033,
      isError: false,
      kind: "result",
      modelEffective: "claude-opus-5-5",
      modelOverridden: false,
      modelRequested: "opus",
      outputTokens: 1449,
      result: "completed",
      subtype: "success",
      usage: {
        cacheReadTokens: 205015,
        cacheWriteTokens: 31002,
        inputTokens: 16,
        outputTokens: 1449,
        toolCalls: 9,
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 35,
    timestamp: 1791248127111,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "status",
      status: "autonomous_turn_started: the agent began a turn nobody prompted",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 36,
    timestamp: 1791248127247,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "assistant_text",
      text: "Background job finished. Step 7: running the 150-second quiet job.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 37,
    timestamp: 1791248129279,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command:
            'cat "/private/tmp/claude-502/tasks/bi9cros3k.output"; python3 -c "import time; time.sleep(150); print(\'quiet-done\')"',
        },
        toolId: "toolu_01CToJpNgQdDn3twuM5eXvTd",
        toolKind: "execute",
        toolName:
          'cat "/private/tmp/claude-502/tasks/bi9cros3k.output"; python3 -c "import time; time.sleep(150); print(\'quiet-done\')"',
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 38,
    timestamp: 1791248279830,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      content: "```console\nbg-done\n\n[exited with code 0]\nquiet-done\n```",
      input: {
        command:
          'cat "/private/tmp/claude-502/tasks/bi9cros3k.output"; python3 -c "import time; time.sleep(150); print(\'quiet-done\')"',
        description: "Show background output, then run 150-second job",
        timeout: 200000,
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_01CToJpNgQdDn3twuM5eXvTd",
      toolKind: "execute",
      toolName:
        'cat "/private/tmp/claude-502/tasks/bi9cros3k.output"; python3 -c "import time; time.sleep(150); print(\'quiet-done\')"',
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 48,
    timestamp: 1791248348734,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "assistant_text",
      text: "Got `c`. Step 9: getting the end time for the summary.",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 49,
    timestamp: 1791248348948,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command: "date -u",
        },
        toolId: "toolu_015LZ7GAZ6VCdjL3sKDxf3vq",
        toolKind: "execute",
        toolName: "date -u",
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 50,
    timestamp: 1791248349261,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      content: "```console\nTue Oct  6 00:59:09 UTC 2026\n```",
      input: {
        command: "date -u",
        description: "Show current UTC time",
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_015LZ7GAZ6VCdjL3sKDxf3vq",
      toolKind: "execute",
      toolName: "date -u",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 51,
    timestamp: 1791248352973,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "assistant_text",
      text: "All nine steps of audit run BK-AUDIT-1006 are done.\n\n**Files created** (in the worktree, not committed):\n- `src/counter.py`: has `count_words(path)` and a `__main__` that prints the count. It gives 12 for the sample file.\n- `notes/sample.txt`: three lines, 12 words in total.\n\n**Change from the subagent (step 5):** it suggested reading the file one line at a time with `sum(len(line.split()) for line in f)` instead of `f.read()`, so large files aren't loaded into memory all at once. I applied it, and the count is still 12.\n\n",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 52,
    timestamp: 1791248353071,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "status",
      status: "autonomous_turn: the agent woke on task-notification",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 53,
    timestamp: 1791248353085,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "assistant_text",
      text: '**Failed commands:** only `python3 src/missing.py`, which failed on purpose (exit 2, "No such file or directory"). Everything else exited 0 and printed what it should: `bg-done`, `quiet-done`, `a`, `b`, `c`.\n\n**Start:** Tue Oct 6 00:53:57 UTC 2026\n**End:** Tue Oct 6 00:59:09 UTC 2026',
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 54,
    timestamp: 1791248353101,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      kind: "context_window_updated",
      usage: {
        size: 1000000,
        used: 32859,
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 55,
    timestamp: 1791248353115,
    turnId: "0c5d7899-6523-416a-9e15-952558130462",
    item: {
      costReason: "no_usage_reported",
      costUsd: null,
      durationMs: 226168,
      isError: false,
      kind: "result",
      modelEffective: null,
      modelReason: "adapter_reported_none",
      modelRequested: "opus",
      result: "completed",
      subtype: "success",
      usage: {
        toolCalls: 5,
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 56,
    timestamp: 1791248698909,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      commandId: "csc-e641cd86-5471-4e11-b069-cfb38e8ad156",
      content:
        "Run this in the foreground and write nothing while it runs:\npython3 -c \"import time; time.sleep(150); print('quiet-done')\"",
      kind: "user_prompt",
      operatorPubkey:
        "3d3b7169a13a8311b480bdfce85b4a0c7ff9b185832cbc6e547db7bbcf96c05e",
      steered: false,
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 57,
    timestamp: 1791248701653,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      kind: "tool_call",
      tool: {
        input: {
          command:
            "python3 -c \"import time; time.sleep(150); print('quiet-done')\"",
        },
        toolId: "toolu_01KDxXc1U9dmoyezwGcTZg7X",
        toolKind: "execute",
        toolName:
          "python3 -c \"import time; time.sleep(150); print('quiet-done')\"",
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 58,
    timestamp: 1791248852744,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      content: "```console\nquiet-done\n```",
      input: {
        command:
          "python3 -c \"import time; time.sleep(150); print('quiet-done')\"",
        description: "Run 150-second sleep job",
        timeout: 200000,
      },
      isError: false,
      kind: "tool_result",
      toolId: "toolu_01KDxXc1U9dmoyezwGcTZg7X",
      toolKind: "execute",
      toolName:
        "python3 -c \"import time; time.sleep(150); print('quiet-done')\"",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 59,
    timestamp: 1791248854343,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      kind: "assistant_text",
      text: "It finished and printed `quiet-done` (exit 0).",
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 60,
    timestamp: 1791248854358,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      kind: "context_window_updated",
      usage: {
        size: 1000000,
        used: 33112,
      },
    },
  },
  {
    target: {
      driver: "claude-agent-acp",
      instanceId: "1958c6c448e05eed",
      sessionId: "becbd0cb-e638-4d00-a07f-a9bc08279aa5",
      generation: 1,
    },
    eventSeq: 61,
    timestamp: 1791248854372,
    turnId: "7bebdbef-8a4b-4b29-ac3c-015dd85afee7",
    item: {
      costBasis: "adapter_estimate",
      costUsd: 0.09348179999999989,
      durationMs: 155695,
      inputTokens: 66022,
      isError: false,
      kind: "result",
      modelEffective: "claude-opus-5-5",
      modelOverridden: false,
      modelRequested: "opus",
      outputTokens: 147,
      result: "completed",
      subtype: "success",
      usage: {
        cacheReadTokens: 65449,
        cacheWriteTokens: 569,
        inputTokens: 4,
        outputTokens: 147,
        toolCalls: 1,
      },
    },
  },
];
