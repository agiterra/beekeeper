# API shape

Two endpoints, both `POST`.

- `/events` — submit a signed event
- `/query` — REQ filters over HTTP

Nothing here is schema-checked: a document artifact may be anything, which
is the whole point of the `docs/` tree being separate from `plans/`.
