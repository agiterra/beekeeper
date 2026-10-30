#!/usr/bin/env python3
"""Fixture-only Git crash injector; production has no fault environment switch."""
import json
import os
import pathlib
import signal
import subprocess
import sys

args = sys.argv[1:]
result = subprocess.run([os.environ['ACME_HIRE_REAL_GIT'], *args])
root = pathlib.Path(os.environ['ACME_HIRE_FAULT_DIR'])
control = root / 'git-fault-mode'
mode = control.read_text().strip() if control.exists() else ''
code = 'worktree' in args and 'add' in args and '--no-checkout' in args
agents = 'clone' in args and '--no-checkout' in args and any('code-wt-builder-' in a and a.endswith('-agents') for a in args)
if result.returncode == 0 and ((mode == 'crash-code' and code) or (mode == 'crash-agents' and agents)):
    (root / (mode + '.json')).write_text(json.dumps({'args': args, 'listener_pid': os.getppid()}))
    os.kill(os.getppid(), signal.SIGKILL)
sys.exit(result.returncode)
