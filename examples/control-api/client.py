"""
Minimal obs-websocket v5 client for Streamtubah's control API.
Any obs-websocket v5 library (obsws-python, obs-websocket-js, …) works the same way.

    pip install websockets
    python client.py --password <API password> GetSceneList
    python client.py --password <pw> SetCurrentProgramScene '{"sceneName": "Scene 2"}'
    python client.py --password <pw> --events        # print events until Ctrl+C
"""
import argparse
import asyncio
import base64
import hashlib
import json

import websockets


def _b64sha(s: str) -> str:
    return base64.b64encode(hashlib.sha256(s.encode()).digest()).decode()


async def connect(url: str, password: str, subscriptions: int = 0x7FF):
    ws = await websockets.connect(url, max_size=None)
    hello = json.loads(await ws.recv())["d"]
    ident = {"rpcVersion": 1, "eventSubscriptions": subscriptions}
    auth = hello.get("authentication")
    if auth:
        ident["authentication"] = _b64sha(_b64sha(password + auth["salt"]) + auth["challenge"])
    await ws.send(json.dumps({"op": 1, "d": ident}))
    msg = json.loads(await ws.recv())
    assert msg["op"] == 2, msg
    return ws


async def request(ws, request_type: str, data: dict | None = None) -> dict:
    rid = f"r{id(data)}"
    await ws.send(json.dumps({"op": 6, "d": {"requestType": request_type, "requestId": rid, "requestData": data or {}}}))
    while True:
        msg = json.loads(await ws.recv())
        if msg["op"] == 7 and msg["d"]["requestId"] == rid:
            return msg["d"]


async def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="ws://127.0.0.1:4455")
    ap.add_argument("--password", default="")
    ap.add_argument("--events", action="store_true")
    ap.add_argument("request", nargs="?")
    ap.add_argument("data", nargs="?", default="{}")
    a = ap.parse_args()
    ws = await connect(a.url, a.password)
    if a.request:
        print(json.dumps(await request(ws, a.request, json.loads(a.data)), indent=2))
    if a.events:
        async for msg in ws:
            print(msg)
    await ws.close()


if __name__ == "__main__":
    asyncio.run(main())
