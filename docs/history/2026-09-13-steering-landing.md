# Steering and setup-authoring landing — September 13

The integration candidate landed on relay and GitHub main as
`e12495c633c2bedfa4510ac86c2e128c834e4eb1`. Both `git ls-remote ... refs/heads/main`
reads returned that exact commit after push; local main advanced by fast-forward.
Andy’s `343ea8bd9` redaction change is preserved below it. Rebase introduced
exactly Andy’s six changed files, with no source conflicts.

Startup corrections were committed with DCO signoff as `702ec22e6`, then
rebased with signoff to `e12495c63`. The completed pre-rebase CI is accounted
for in [the recovered report](2026-09-13-final-ci-recovery.md). The post-rebase
normal pre-push floor and destination/branch/map guards passed in 303.62s;
relay push completed with exit 0. Local evidence:
`../review-2026-09-13-final-ci-sol/landing-push.log`.

Brian accepted the unresolved dense-history browser failure. The selected
browser matrix remains 44 passed and one failed; this landing does not relabel
that result. No additional full smoke run was used to land it.

The new project publication/installation/lead workflow is still a separate
uncommitted candidate under focused review. This landing includes setup draft,
authoring, validation and snapshots, not that activation workflow. Installed
Mac rebuild and live UI acceptance remain next; no production deployment was
performed.
