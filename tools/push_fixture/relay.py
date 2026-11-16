"""Loopback byte relays for Porto's network-none fixture.

TLS terminates only in Caddy. This module never parses TLS, changes trust,
contacts the Internet, or forwards to a caller-selected destination.
"""

import asyncio
import contextlib
import datetime as dt
import json
import os
import signal
import subprocess

import fixture


async def serve(value: dict) -> None:
    containers = fixture.owned_containers(value)
    proxy = next(
        item for item in containers
        if item["Config"]["Labels"]["com.docker.compose.service"] == "proxy"
    )
    fixture.verify_network_none(value, containers)
    control_path = fixture.WORK / "run" / "relay-control.sock"
    metadata_path = fixture.WORK / "relay.json"
    if control_path.exists() or metadata_path.exists():
        raise fixture.FixtureError("Existing relay ownership must be inspected, not overwritten")
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for signum in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(signum, stop.set)
    tasks = set()
    children = set()

    async def forward(reader, writer, port):
        if len(tasks) >= 32:
            writer.close()
            return
        task = asyncio.current_task()
        tasks.add(task)
        process = None
        try:
            current = fixture.state()
            if current["run_id"] != value["run_id"]:
                raise fixture.FixtureError("Relay run ownership changed")
            identity = next(
                item["id"] for item in current["containers"] if item["service"] == "proxy"
            )
            process = await asyncio.create_subprocess_exec(
                "docker", "--context", "porto", "exec", "-i", identity,
                "nc", "127.0.0.1", str(port),
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL,
            )
            children.add(process)

            async def upload():
                while data := await reader.read(32768):
                    process.stdin.write(data)
                    await process.stdin.drain()
                process.stdin.close()

            async def download():
                while data := await process.stdout.read(32768):
                    writer.write(data)
                    await writer.drain()

            streams = [asyncio.create_task(upload()), asyncio.create_task(download())]
            try:
                await asyncio.wait(streams, return_when=asyncio.FIRST_COMPLETED)
            finally:
                for stream in streams:
                    stream.cancel()
                await asyncio.gather(*streams, return_exceptions=True)
        except (ConnectionError, OSError):
            pass
        finally:
            writer.close()
            with contextlib.suppress(ConnectionError, OSError):
                await writer.wait_closed()
            if process:
                if process.stdin:
                    process.stdin.close()
                if process.returncode is None:
                    with contextlib.suppress(ProcessLookupError):
                        process.terminate()
                with contextlib.suppress(asyncio.TimeoutError):
                    await asyncio.wait_for(process.wait(), 5)
                children.discard(process)
            tasks.discard(task)

    async def control(reader, writer):
        try:
            message = json.loads(await asyncio.wait_for(reader.readline(), 3))
            if message == {"action": "stop", "run_id": value["run_id"]}:
                writer.write(b"stopping\n")
                await writer.drain()
                stop.set()
        except (ValueError, asyncio.TimeoutError):
            pass
        finally:
            writer.close()

    listeners = []
    try:
        for host_port, port in [
            (value["connection_port"], 8443), (value["publishing_port"], 8444)
        ]:
            listeners.append(await asyncio.start_server(
                lambda reader, writer, port=port: forward(reader, writer, port),
                "127.0.0.1", host_port, reuse_address=False, limit=65536,
            ))
        listeners.append(await asyncio.start_unix_server(
            control, os.path.relpath(control_path), limit=4096
        ))
        control_path.chmod(0o600)
        fixture.write_json(
            metadata_path,
            {
                "pid": os.getpid(), "run_id": value["run_id"],
                "started_at": dt.datetime.now(dt.UTC).isoformat(),
                "process_start": subprocess.run(
                    ["ps", "-p", str(os.getpid()), "-o", "lstart="],
                    capture_output=True, text=True, check=True,
                ).stdout.strip(),
                "source_sha256": fixture.digest(fixture.HERE / "relay.py"),
            },
            private=True,
        )
        result = await asyncio.to_thread(fixture.ready, value, timeout=60)
        print(json.dumps(result, indent=2), flush=True)
        await stop.wait()
    finally:
        for listener in listeners:
            listener.close()
        await asyncio.gather(*(listener.wait_closed() for listener in listeners))
        for task in list(tasks):
            task.cancel()
        if tasks:
            await asyncio.gather(*list(tasks), return_exceptions=True)
        for child in list(children):
            if child.returncode is None:
                with contextlib.suppress(ProcessLookupError):
                    child.terminate()
                with contextlib.suppress(asyncio.TimeoutError):
                    await asyncio.wait_for(child.wait(), 5)
        if control_path.exists():
            control_path.unlink()
        if metadata_path.exists():
            metadata = fixture.read_json(metadata_path)
            if metadata.get("pid") == os.getpid() and metadata.get("run_id") == value["run_id"]:
                metadata_path.unlink()
