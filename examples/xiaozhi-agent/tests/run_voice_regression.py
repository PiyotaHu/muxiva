"""Run the voice regression gate before deploying the Raspberry Pi service."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys


REPO = pathlib.Path(__file__).resolve().parents[3]
PYTHON = (
    REPO / ".venv" / "bin" / "python"
    if (REPO / ".venv" / "bin" / "python").is_file()
    else pathlib.Path(sys.executable)
)
PI_AGENT_CANDIDATES = (
    REPO.parent / "muxiva-pi-agent",
    REPO / "examples" / "xiaozhi-agent" / ".muxiva" / "agents" / "muxiva-pi-agent",
)


def run(label: str, command: list[str], cwd: pathlib.Path) -> None:
    print(f"\n[voice-regression] {label}", flush=True)
    subprocess.run(command, cwd=cwd, check=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--live",
        action="store_true",
        help="also run the credentialed three-turn ASR/Agent/TTS/WebSocket test",
    )
    args = parser.parse_args()

    run(
        "Qwen ASR/TTS turn, filler, endpointing and barge-in cases",
        [str(PYTHON), "-m", "unittest", "discover", "-s", "providers/algorithm/qwen/python/tests", "-v"],
        REPO,
    )
    run(
        "Xiaozhi live-mic, pacing, queue-tail and stop ordering cases",
        [str(PYTHON), "-m", "unittest", "discover", "-s", "providers/transport/xiaozhi/python/tests", "-v"],
        REPO,
    )
    run(
        "deployed graph stability contract",
        [str(PYTHON), "-m", "unittest", "discover", "-s", "examples/xiaozhi-agent/tests", "-p", "test_voice_regression_contract.py", "-v"],
        REPO,
    )
    pi_agent_repo = next((path for path in PI_AGENT_CANDIDATES if path.is_dir()), None)
    if pi_agent_repo is None:
        raise RuntimeError(f"missing Pi Agent repository; checked: {PI_AGENT_CANDIDATES}")
    run("Pi Agent routing, tools and voice-text cases", ["npm", "test"], pi_agent_repo)
    run("Pi Agent type contract", ["npm", "run", "check"], pi_agent_repo)
    if args.live:
        run(
            "live three-turn conversation with real mid-playback interruption",
            [str(PYTHON), "-m", "unittest", "tests.test_full_duplex", "-v"],
            REPO / "examples" / "xiaozhi-agent",
        )
    print("\n[voice-regression] PASS", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
