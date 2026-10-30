"""Loopback-only transparent fault proxy for the deterministic hire fixture.

HTTP authority remains bound to the public URL. WebSocket bytes pass unchanged.
Control files affect only signed hire creates and authority transitions, never
ordinary setup. Captured events are public signed wire data, not credentials.
"""
import asyncio
import json
import pathlib
import sys


async def serve(front, back, directory):
    root = pathlib.Path(directory)

    async def connection(reader, writer):
        upstream = None
        try:
            header = await reader.readuntil(b"\r\n\r\n")
            fields = dict(line.split(b":", 1) for line in header.split(b"\r\n")[1:] if b":" in line)
            fields = {k.strip().lower(): v.strip() for k, v in fields.items()}
            body = await reader.readexactly(int(fields.get(b"content-length", b"0")))
            mode = (root / "proxy-mode").read_text().strip() if (root / "proxy-mode").exists() else "pass"
            event = None
            if header.startswith(b"POST /events "):
                value = json.loads(body)
                event = value.get("event", value)
            is_create = event and event.get("kind") == 44221 and json.loads(event["content"]).get("action", {}).get("hireRef")
            is_grant = event and event.get("kind") == 44228
            is_refusal = event and event.get("kind") == 44220 and json.loads(event["content"]).get("action", {}).get("text", "").startswith("hire refused: ")
            if is_refusal and mode == "lose-refusal-ack":
                (root / "proxy-refusal.json").write_text(json.dumps(event))
            if is_create and mode in ("hold-create", "lose-create-ack"):
                (root / "proxy-create.json").write_text(json.dumps(event))
            if is_create and mode == "hold-create":
                while (root / "proxy-mode").read_text().strip() == mode:
                    if reader.at_eof():
                        return
                    await asyncio.sleep(0.05)
                return  # The interrupted listener must retry its original bytes.
            if is_grant and mode == "reject-grant":
                (root / "proxy-grant.json").write_text(json.dumps(event))
                writer.write(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                await writer.drain()
                return
            remote, upstream = await asyncio.open_connection("127.0.0.1", back)
            if fields.get(b"upgrade", b"").lower() != b"websocket":
                lines = header.split(b"\r\n")
                header = b"\r\n".join([line for line in lines[:-2] if not line.lower().startswith(b"connection:")] + [b"Connection: close", b"", b""])
            upstream.write(header + body)
            await upstream.drain()
            if (is_create and mode == "lose-create-ack") or (is_grant and mode == "lose-grant-ack") or (is_refusal and mode == "lose-refusal-ack"):
                await remote.read(65536)
                (root / ("proxy-ack-lost" if is_create else "proxy-refusal-ack-lost" if is_refusal else "proxy-grant-ack-lost")).write_text(event["id"])
                return

            async def pump(source, destination):
                while data := await source.read(65536):
                    destination.write(data)
                    await destination.drain()
            tasks = [asyncio.create_task(pump(reader, upstream)), asyncio.create_task(pump(remote, writer))]
            done, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
        except (asyncio.IncompleteReadError, ConnectionError, json.JSONDecodeError):
            pass
        finally:
            if upstream:
                upstream.close()
            writer.close()
    server = await asyncio.start_server(connection, "127.0.0.1", front)
    async with server:
        await server.serve_forever()


asyncio.run(serve(int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]))
