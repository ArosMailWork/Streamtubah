"""
Example voice processor for the Streamtubah Audio Bridge.

    pip install websockets
    python voice_fx.py --password <API password> --effect robot

Effects: passthrough, robot (ring modulation), echo, radio.
Replace `make_effect` with your own DSP / AI voice model.
"""
import argparse
import array
import math

from streamtubah_bridge import run

RATE = 48000


def make_effect(name: str):
    if name == "robot":
        phase = [0.0]
        step = 2 * math.pi * 55 / RATE

        def robot(x: array.array) -> array.array:
            out = array.array("f", x)
            for i in range(0, len(x), 2):
                m = math.sin(phase[0])
                phase[0] += step
                out[i] *= m
                out[i + 1] *= m
            phase[0] %= 2 * math.pi
            return out
        return robot

    if name == "echo":
        delay = int(0.25 * RATE) * 2
        buf = array.array("f", [0.0] * delay)
        pos = [0]

        def echo(x: array.array) -> array.array:
            out = array.array("f", x)
            for i in range(len(x)):
                y = x[i] + 0.45 * buf[pos[0]]
                buf[pos[0]] = y
                out[i] = y
                pos[0] = (pos[0] + 1) % delay
            return out
        return echo

    if name == "radio":
        state = [0.0, 0.0, 0.0, 0.0]  # per channel: high-pass memory, low-pass memory

        def radio(x: array.array) -> array.array:
            out = array.array("f", x)
            for i in range(len(x)):
                c = (i % 2) * 2
                hp = x[i] - state[c]
                state[c] += 0.05 * hp           # remove lows
                state[c + 1] += 0.35 * (hp - state[c + 1])  # remove highs
                out[i] = max(-0.3, min(0.3, state[c + 1] * 2.5))
            return out
        return radio

    return lambda x: x


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--password", default="")
    ap.add_argument("--channel", default="voice")
    ap.add_argument("--port", type=int, default=4455)
    ap.add_argument("--effect", default="robot", choices=["passthrough", "robot", "echo", "radio"])
    a = ap.parse_args()
    run(make_effect(a.effect), channel=a.channel, password=a.password, port=a.port)
