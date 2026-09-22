"""Static server for the Playwright E2E preview build.

Replaces `python3 -m http.server -d dist`, whose default listen backlog is
five connections (`socketserver.TCPServer.request_queue_size`). The desktop
bundle is code-split into ~530 chunks and a cold page load opens far more
than five sockets at once, so the surplus were reset: Chromium reported
`ERR_CONNECTION_RESET` and `Failed to fetch dynamically imported module
.../HomeScreen-*.js`, and the route never mounted. Every locator in the spec
then failed with "element(s) not found" — indistinguishable from the feature
under test being broken, and reproducible across retries because the chunk
count, not timing, is what overflows the queue.

Threading is already the default for `http.server` on Python 3.7+; only the
backlog was short.
"""

import argparse
import functools
import http.server
import socketserver

# Deep enough for a cold load of the whole split bundle with room to spare.
socketserver.TCPServer.request_queue_size = 512


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--directory", default="dist")
    parser.add_argument("--bind", default="127.0.0.1")
    args = parser.parse_args()
    http.server.test(
        HandlerClass=functools.partial(
            http.server.SimpleHTTPRequestHandler, directory=args.directory
        ),
        ServerClass=http.server.ThreadingHTTPServer,
        port=args.port,
        bind=args.bind,
    )


if __name__ == "__main__":
    main()
