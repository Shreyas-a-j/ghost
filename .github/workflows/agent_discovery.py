"""Check what each pinned client sends to a loopback model endpoint."""
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading

agent = sys.argv[1]
marker = "ghost-fixture-discovery-7c862f"
observed = threading.Event()


class Probe(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        if marker.encode() in body and b"ghost resume" in body:
            observed.set()
        self.send_response(400)
        self.end_headers()
        self.wfile.write(b'{"error":{"message":"fixture stops at model input"}}')

    def log_message(self, *args):
        pass


with tempfile.TemporaryDirectory() as temp, http.server.ThreadingHTTPServer(("127.0.0.1", 0), Probe) as server:
    port = server.server_address[1]
    endpoint = f"http://127.0.0.1:{port}"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    root = Path(temp) / "repository"
    root.mkdir()
    home = Path(temp) / "home"
    home.mkdir()
    env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / "config"),
               CI="1", DISABLE_TELEMETRY="1", DISABLE_NON_ESSENTIAL_MODEL_CALLS="1")
    def run(*cmd):
        subprocess.run(cmd, cwd=root, env=env, check=True)

    ghost = str(Path("target/debug/ghost").resolve())
    run("git", "init", "-q")
    run(ghost, "init", "--agents", agent)
    run(ghost, "sync", "--apply")
    guide = root / ("CLAUDE.md" if agent == "claude" else "AGENTS.md")
    with guide.open("a") as f:
        f.write("\n" + marker + "\n")

    if agent == "claude":
        env.update(ANTHROPIC_BASE_URL=endpoint, ANTHROPIC_API_KEY="local-fixture")
        cmd = ["claude", "-p", "Say hello", "--model", "claude-sonnet-4-5"]
    elif agent == "codex":
        cmd = ["codex", "exec", "--skip-git-repo-check", "-c", 'model_provider="fixture"',
               "-c", 'model_providers.fixture.name="Fixture"', "-c",
               f'model_providers.fixture.base_url="{endpoint}/v1"', "-c",
               'model_providers.fixture.wire_api="responses"', "-m", "gpt-5", "Say hello"]
    else:
        config = {"provider": {"fixture": {"npm": "@ai-sdk/openai-compatible", "name": "Fixture",
                  "options": {"baseURL": endpoint + "/v1", "apiKey": "local-fixture"},
                  "models": {"local": {"name": "Fixture"}}}}}
        (root / "opencode.json").write_text(json.dumps(config))
        cmd = ["opencode", "run", "-m", "fixture/local", "Say hello"]
    process = subprocess.Popen(cmd, cwd=root, env=env, start_new_session=True)
    try:
        for _ in range(90):
            if observed.wait(1) or process.poll() is not None:
                break
        if not observed.is_set():
            raise SystemExit(f"{agent}: generated instruction missing from captured model request")
        print(f"{agent}: verified project instruction discovery")
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
        process.wait()
