"""
Minimal client library for the Streamtubah Audio Bridge.

Add the "Audio Bridge (external app)" filter to any audio source in Streamtubah,
then connect with:

    from streamtubah_bridge import run
    run(process, channel="voice", password="...")

`process(samples)` receives a Python `array('f')` of interleaved stereo float32
samples (L, R, L, R, ...) at 48 kHz and must return the same number of samples.
Protocol details: docs/audio-bridge.md
"""
import array
import asyncio
import json
import sys
from typing import Callable

import websockets  # pip install websockets

Process = Callable[[array.array], array.array]


async def _session(url: str, process: Process) -> None:
    async with websockets.connect(url, max_size=None) as ws:
        hello = json.loads(await ws.recv())
        print(f"connected: {hello}", file=sys.stderr)
        async for msg in ws:
            if isinstance(msg, (bytes, bytearray)):
                block = array.array("f")
                block.frombytes(msg)
                if sys.byteorder != "little":
                    block.byteswap()
                out = process(block)
                if sys.byteorder != "little":
                    out.byteswap()
                await ws.send(out.tobytes())


def run(process: Process, channel: str = "voice", password: str = "",
        host: str = "127.0.0.1", port: int = 4455) -> None:
    """Connects and keeps processing until interrupted; reconnects on errors."""
    url = f"ws://{host}:{port}/audio/{channel}?password={password}"

    async def loop() -> None:
        while True:
            try:
                await _session(url, process)
            except (OSError, websockets.ConnectionClosed) as e:
                print(f"disconnected ({e}); retrying in 2 s", file=sys.stderr)
                await asyncio.sleep(2)

    try:
        asyncio.run(loop())
    except KeyboardInterrupt:
        pass
