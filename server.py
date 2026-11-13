"""Live dashboard for a Chia PoS2 testnet farm.

Run it with the chia-blockchain venv active and CHIA_ROOT pointing at your testnet:
    cd chia-blockchain && . ./activate
    export CHIA_ROOT=~/.chia/<network_name>
    python path/to/server.py --open
"""

from __future__ import annotations

import argparse
import json
import os
import re
import ssl
import threading
import time
import urllib.request
from collections import deque
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

try:
    import psutil
    import yaml
    from chia_rs import PlotParam, ProofOfSpace
    from chia_rs.sized_bytes import bytes32

    from chia.consensus.constants import replace_str_to_bytes
    from chia.consensus.default_constants import DEFAULT_CONSTANTS, update_testnet_overrides
    from chia.types.blockchain_format.proof_of_space import (
        calculate_plot_filter_bits,
        calculate_prefix_bits,
        max_plot_strength,
        passes_plot_filter,
        passes_plot_filter_v2,
        v1_cut_off_height,
    )
    from chia.util.bech32m import decode_puzzle_hash
except ImportError as e:
    raise SystemExit(
        f"Can't load Chia ({e}).\n"
        "Run this with the venv of a chia-blockchain checkout on the hardfork-testnet branch:\n"
        "  cd chia-blockchain && . ./activate"
    ) from None

parser = argparse.ArgumentParser(description="Live dashboard for a Chia PoS2 testnet farm.")
parser.add_argument("--root", default=os.environ.get("CHIA_ROOT", "~/.chia/mainnet"), help="Chia root folder (default: $CHIA_ROOT)")
parser.add_argument("--port", type=int, default=int(os.environ.get("DASH_PORT", "8899")), help="port to serve on (default: 8899)")
parser.add_argument("--open", action="store_true", help="open the dashboard in your browser")
ARGS = parser.parse_args() if __name__ == "__main__" else parser.parse_args([])

ROOT = Path(ARGS.root).expanduser()
_config_path = ROOT / "config/config.yaml"
if not _config_path.exists():
    raise SystemExit(
        f"No Chia config at {_config_path}.\n"
        "Point CHIA_ROOT at your testnet folder, for example: export CHIA_ROOT=~/.chia/<network_name>"
    )
CONFIG = yaml.safe_load(_config_path.read_text())
HERE = Path(__file__).resolve().parent
PORT = ARGS.port
POLL_SECONDS = 2.0
HISTORY_EVERY = 10.0
SECONDS_PER_BLOCK = 24 * 3600 / 4608  # same fallback as `chia farm summary`
LOG_PATH = ROOT / "log/debug.log"

# Same constants the full node runs with (see chia/full_node/start_full_node.py).
_overrides = dict(CONFIG["full_node"]["network_overrides"]["constants"][CONFIG["selected_network"]])
update_testnet_overrides(CONFIG["selected_network"], _overrides)
CONSTANTS = replace_str_to_bytes(DEFAULT_CONSTANTS, **_overrides)
FORK_HEIGHT = int(CONSTANTS.HARD_FORK2_HEIGHT)
V1_CUTOFF = v1_cut_off_height(CONSTANTS)

PEER_TYPES = {1: "full node", 2: "harvester", 3: "farmer", 4: "timelord", 5: "introducer", 6: "wallet", 7: "data layer"}


def _ssl_context(service: str) -> ssl.SSLContext:
    ctx = ssl.create_default_context(cafile=str(ROOT / "config/ssl/ca/private_ca.crt"))
    ctx.check_hostname = False
    # Python 3.13+ turns on strict X.509 checks, which Chia's certificates fail ("Missing Authority Key
    # Identifier"). Chia's own RPC client doesn't use them either; the chain is still verified against the CA.
    ctx.verify_flags &= ~ssl.VERIFY_X509_STRICT
    ssl_dir = ROOT / "config/ssl" / service
    ctx.load_cert_chain(ssl_dir / f"private_{service}.crt", ssl_dir / f"private_{service}.key")
    return ctx


SERVICES = {name: (CONFIG[name]["rpc_port"], _ssl_context(name)) for name in ("full_node", "farmer", "harvester", "wallet")}


def rpc(service: str, endpoint: str, payload: dict | None = None) -> dict | None:
    port, ctx = SERVICES[service]
    req = urllib.request.Request(
        f"https://127.0.0.1:{port}/{endpoint}",
        data=json.dumps(payload or {}).encode(),
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, context=ctx, timeout=5) as resp:
            body = json.load(resp)
    except (OSError, ValueError):
        return None
    return body if body.get("success") else None


def plot_class(param: PlotParam) -> str:
    """"v1", or "s<strength>" for v2 plots."""
    return "v1" if param.size_v1 is not None else f"s{param.strength_v2}"


SP_RE = re.compile(
    r"^(\S+) \S+ harvester \S+\s*: INFO\s+(?:challenge_hash: (\S+) \.\.\.)?(\d+) plots were eligible.*?"
    r"Found (\d+) V1 proofs and (\d+) V2 qualities\. Time: ([\d.]+) s\. Total (\d+) plots"
)
FARMED_RE = re.compile(r"^(\S+) .*Farmed unfinished_block (\w+), SP: (\d+)")
LEVEL_RE = re.compile(r"^(\S+) \S+ (\S+) (\S+)\s*: (WARNING|ERROR|CRITICAL)\s+(.*)")
NOISE_RE = re.compile(r"upnp|portmapping|mempool using the slow-path", re.I)


PLOT_SUFFIXES = (".plot", ".gplot")
PHASE_RE = re.compile(r"\.p(\d)\.")


def _flag(args: list[str], short: str, long: str, default: str | None = None) -> str | None:
    for i, a in enumerate(args[:-1]):
        if a in (short, long):
            return args[i + 1]
    return default


def _mtime(f: Path) -> float | None:
    try:
        return f.stat().st_mtime
    except OSError:
        return None


def plotting_status(loaded: set[str] | None) -> dict:
    """Running `chia plots create`/`create2` jobs, and plots on disk versus plots the harvester has loaded.

    Plot directories are re-read from config each time, so ones added with `chia plots add` show up.
    """
    jobs: dict[tuple, dict] = {}
    for p in psutil.process_iter(["name", "cmdline", "cwd", "create_time"]):
        cmd = p.info["cmdline"] or []
        if "python" not in (p.info["name"] or "").lower() or "plots" not in cmd:
            continue
        args = cmd[cmd.index("plots") + 1 :]
        if not args or args[0] not in ("create", "create2"):
            continue
        key = tuple(args)
        if key in jobs and jobs[key]["started"] <= p.info["create_time"]:
            continue  # the venv launcher and its child share a command line; keep the older one
        cwd = Path(p.info["cwd"] or ".")
        jobs[key] = {
            "kind": "v2" if args[0] == "create2" else "v1",
            "k": _flag(args, "-k", "--size"),
            "total": int(_flag(args, "-n", "--num", "1")),
            "started": p.info["create_time"],
            "final": cwd / _flag(args, "-d", "--final_dir", "."),
            "tmp": cwd / _flag(args, "-t", "--tmp_dir", "."),
        }
    out = []
    for j in jobs.values():
        # The plotter creates and deletes temp files constantly, so any file can vanish between glob and stat.
        times = {f: t for f in {*j["final"].glob("*"), *j["tmp"].glob("*.tmp")} if (t := _mtime(f)) is not None}
        done = [f for f, t in times.items() if f.suffix in PLOT_SUFFIXES and t >= j["started"]]
        tmps = sorted((f for f in times if f.suffix == ".tmp"), key=times.get)
        phased = [m for f in tmps if (m := PHASE_RE.search(f.name))]
        phase = int(phased[-1][1]) if phased else None
        last = max((times[f] for f in tmps + done), default=None)
        out.append({
            "kind": j["kind"], "k": j["k"], "total": j["total"], "done": len(done), "started": j["started"],
            "phase": phase, "last_write": last, "dir": str(j["final"]),
        })

    try:
        dirs = yaml.safe_load(_config_path.read_text())["harvester"].get("plot_directories") or []
    except (OSError, KeyError, yaml.YAMLError):
        dirs = []
    on_disk = {"v1": 0, "v2": 0}
    names = set()
    for d in dirs:
        for f in Path(d).glob("*"):
            if f.suffix in PLOT_SUFFIXES:
                on_disk["v2" if f.suffix == ".gplot" else "v1"] += 1
                names.add(f.name)
    return {
        "jobs": sorted(out, key=lambda j: j["started"]),
        "on_disk": on_disk,
        "not_loaded": len(names - loaded) if loaded is not None else None,
    }


def log_time(stamp: str) -> float:
    try:
        return datetime.fromisoformat(stamp).timestamp()
    except ValueError:
        return time.time()


STATS_PATH = HERE / f"strength-stats-{CONFIG['selected_network']}.json"
SPS_PER_DAY = int(CONSTANTS.NUM_SPS_SUB_SLOT) * 86400 / int(CONSTANTS.SUB_SLOT_TIME_TARGET)
GENESIS = CONSTANTS.GENESIS_CHALLENGE.hex()


def _hex(s: str) -> str:
    return s.removeprefix("0x").lower()


def unlock_height(strength: int) -> int:
    """First transaction height at which a v2 plot of this strength is farmed (the fork, or later for high strengths)."""
    candidates = sorted({FORK_HEIGHT} | {FORK_HEIGHT + int(o) for o in CONSTANTS.PLOT_FILTER_V2_RELATIVE_HEIGHT})
    return next((h for h in candidates if max_plot_strength(h, CONSTANTS) >= strength), -1)


class StrengthStats:
    """Filter passes, proofs and wins per plot strength, to check that strength behaves as designed.

    The harvester only logs totals per signage point, so every plot's filter result is recomputed here with the
    same functions and gating the harvester uses (chia/harvester/harvester_api.py). As a check, our totals are
    compared with the harvester's log for each finished sub-slot. Counts are saved to STATS_PATH across restarts.
    """

    def __init__(self) -> None:
        self.counts: dict[str, dict] = {}
        self.seen_sps: dict[str, None] = {}
        self.seen_proofs: dict[str, None] = {}
        self.since = time.time()
        self.prev_challenge: dict[str, str] = {}
        self.ours: dict[str, list] = {}  # challenge prefix -> [signage points, passes, complete]
        self.theirs: dict[str, list] = {}  # challenge prefix -> [signage points, eligible plots], from the log
        self.current_prefix: str | None = None
        self.last_save = 0.0
        try:
            saved = json.loads(STATS_PATH.read_text())
            self.counts, self.since = saved["counts"], saved["since"]
            self.seen_sps = dict.fromkeys(saved["seen_sps"])
            self.seen_proofs = dict.fromkeys(saved["seen_proofs"])
        except (OSError, ValueError, KeyError):
            pass

    def _c(self, cls: str) -> dict:
        return self.counts.setdefault(cls, {"checks": 0, "locked": 0, "passes": 0, "expected": 0.0, "proofs": 0, "gb_sps": 0.0})

    def _prev(self, challenge: str) -> str | None:
        """Challenge of the sub-slot before the one with this challenge."""
        if challenge == GENESIS:
            return None
        if challenge not in self.prev_challenge:
            r = rpc("full_node", "get_recent_signage_point_or_eos", {"challenge_hash": challenge})
            if r is None:
                return None
            self.prev_challenge[challenge] = _hex(r["eos"]["challenge_chain"]["challenge_chain_end_of_slot_vdf"]["challenge"])
        return self.prev_challenge[challenge]

    def _filter_challenge(self, challenge: str, index: int) -> str | None:
        """As FullNodeStore.get_filter_challenge: SPs in the first window use SS(n-2), later ones SS(n-1)."""
        prev = self._prev(challenge)
        if index >= int(CONSTANTS.FILTER_WINDOW_SIZE) or prev is None:
            return prev
        return self._prev(prev)

    def harvester_sp(self, challenge_prefix: str, eligible: int) -> None:
        t = self.theirs.setdefault(challenge_prefix.lower(), [0, 0])
        t[0] += 1
        t[1] += eligible

    def update(self, sps: list[dict], plots: list[dict], peak: int, last_tx: int) -> None:
        for entry in sps:
            sp = entry["signage_point"]
            key = _hex(sp["challenge_chain_sp"])
            for ident, proof in entry.get("proofs", []):
                pkey = f"{key}:{ident}"
                if pkey not in self.seen_proofs:
                    self.seen_proofs[pkey] = None
                    self._c(plot_class(ProofOfSpace.from_json_dict(proof).param()))["proofs"] += 1
            if key in self.seen_sps:
                continue
            self.seen_sps[key] = None
            self._check_sp(_hex(sp["challenge_hash"]), key, int(sp["signage_point_index"]), plots, peak, last_tx)
        for d in (self.seen_sps, self.seen_proofs):
            for k in list(d)[: max(0, len(d) - 3000)]:
                del d[k]
        if time.time() - self.last_save > 30:
            self.last_save = time.time()
            STATS_PATH.write_text(json.dumps({
                "counts": self.counts, "since": self.since, "seen_sps": list(self.seen_sps), "seen_proofs": list(self.seen_proofs),
            }))

    def _check_sp(self, challenge: str, sp_hash: str, index: int, plots: list[dict], peak: int, last_tx: int) -> None:
        passes, complete, fc = 0, True, None
        for p in plots:
            c = self._c(p["cls"])
            if p["cls"] == "v1":
                if last_tx >= V1_CUTOFF:
                    continue
                bits = calculate_prefix_bits(CONSTANTS, peak, p["param"])
                passed = passes_plot_filter(bits, p["id"], bytes32.fromhex(challenge), bytes32.fromhex(sp_hash))
            else:
                if last_tx < FORK_HEIGHT:
                    continue
                if p["strength"] > max_plot_strength(last_tx, CONSTANTS):
                    c["locked"] += 1
                    continue
                fc = fc or self._filter_challenge(challenge, index)
                if fc is None:  # can't reproduce this one; leave it out rather than guess
                    complete = False
                    continue
                bits = calculate_plot_filter_bits(last_tx, CONSTANTS, p["strength"])
                passed = passes_plot_filter_v2(
                    p["id"], p["meta_group"], bits, bytes32.fromhex(fc), index, int(CONSTANTS.FILTER_WINDOW_SIZE)
                )
            c["checks"] += 1
            c["gb_sps"] += p["gb"]
            c["expected"] += 2.0 ** -bits
            c["passes"] += passed
            passes += passed
        prefix = challenge[:10]
        self.current_prefix = prefix
        o = self.ours.setdefault(prefix, [0, 0, True])
        o[0] += 1
        o[1] += passes
        o[2] = o[2] and complete

    def check(self) -> dict:
        """Compare with the harvester per finished sub-slot where both saw the same signage points."""
        compared, mismatches = 0, []
        for prefix, (n, passes, complete) in self.ours.items():
            t = self.theirs.get(prefix)
            if prefix == self.current_prefix or not complete or t is None or t[0] != n:
                continue
            compared += 1
            if t[1] != passes:
                mismatches.append({"challenge": prefix, "ours": passes, "harvester": t[1], "sps": n})
        return {"compared": compared, "mismatches": mismatches[-5:], "mismatch_count": len(mismatches)}

    def snapshot(self, plots: list[dict], chain: list[dict], last_tx: int | None) -> dict:
        present = {p["cls"] for p in plots}
        classes = sorted(present | set(self.counts), key=lambda c: (c != "v1", int(c[1:]) if c != "v1" else 0))
        rows = []
        for cls in classes:
            c = self._c(cls)
            row = {
                **c, "cls": cls, "plots": sum(p["cls"] == cls for p in plots),
                "gb": sum(p["gb"] for p in plots if p["cls"] == cls),
                "wins": sum(b["ours"] and b.get("cls") == cls for b in chain),
            }
            if cls == "v1":
                row["state"] = "retired" if last_tx is not None and last_tx >= V1_CUTOFF else "farming"
                row["until"] = V1_CUTOFF
                v1 = next((p for p in plots if p["cls"] == "v1"), None)
                row["bits"] = calculate_prefix_bits(CONSTANTS, last_tx or 0, v1["param"]) if v1 else None
            else:
                plot_strength = int(cls[1:])
                row["unlock"] = unlock_height(plot_strength)
                row["bits"] = calculate_plot_filter_bits(last_tx or 0, CONSTANTS, plot_strength)
                if last_tx is None or last_tx < FORK_HEIGHT:
                    row["state"] = "before fork"
                elif plot_strength > max_plot_strength(last_tx, CONSTANTS):
                    row["state"] = "locked"
                else:
                    row["state"] = "farming"
            rows.append(row)
        return {
            "rows": rows, "since": self.since, "sps_per_day": SPS_PER_DAY, "last_tx": last_tx, "fork": FORK_HEIGHT,
            "max_strength": max_plot_strength(last_tx or 0, CONSTANTS), "check": self.check(),
        }


strength = StrengthStats()


def plots_for_stats(plot_list: dict | None) -> list[dict]:
    out = []
    for p in (plot_list or {}).get("plots", []):
        v2 = bool(p["size"] & 0x80)
        out.append({
            "cls": f"s{p['size'] & 0x7F}" if v2 else "v1",
            "strength": p["size"] & 0x7F if v2 else None,
            "id": bytes32.fromhex(_hex(p["plot_id"])),
            "meta_group": int(p.get("meta_group") or 0),
            "param": PlotParam.make_v1(p["size"]) if not v2 else None,
            "gb": p["file_size"] / 1e9,
        })
    return out


def expected_pass(plots: list[dict], height: int) -> float:
    """Chance that at least one plot passes the filter at a signage point, at this height."""
    miss = 1.0
    for plot in plots:
        if plot["size"] & 0x80:  # v2: 0x80 | strength, and only eligible from the fork
            if height < FORK_HEIGHT:
                continue
            bits = calculate_plot_filter_bits(height, CONSTANTS, plot["size"] & 0x7F)
        else:
            if height >= V1_CUTOFF:
                continue
            bits = calculate_prefix_bits(CONSTANTS, height, PlotParam.make_v1(plot["size"]))
        miss *= 1 - 2.0**-bits
    return 1 - miss


class Collector:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.snapshot: dict = {"ready": False}
        self.blocks: dict[int, dict] = {}
        self.kind_order: list[str] = ["v1", "v2"]
        self.history: deque = deque(maxlen=720)
        self.last_history = 0.0
        self.sps: deque = deque(maxlen=300)
        self.events: deque = deque(maxlen=60)
        self.pending_proofs: list[dict] = []  # held back until we know whether they became a block
        self.farmed_times: list[float] = []
        self.sp_totals = {"count": 0, "eligible": 0, "passed": 0, "proofs": 0, "v1": 0, "v2": 0, "max_s": 0.0, "sum_s": 0.0}
        self.log_offset: int | None = None
        self.log_inode: int | None = None
        self.our_ph: str | None = None
        self.started = time.time()

    # ---- blocks -------------------------------------------------------
    def _ingest_blocks(self, start: int, end: int) -> None:
        resp = rpc("full_node", "get_blocks", {"start": start, "end": end, "exclude_header_hash": True})
        if resp is None:
            return
        for b in resp["blocks"]:
            h = b["reward_chain_block"]["height"]
            cls = plot_class(ProofOfSpace.from_json_dict(b["reward_chain_block"]["proof_of_space"]).param())
            kind = "v1" if cls == "v1" else "v2"
            ftb = b.get("foliage_transaction_block")
            farmer_ph = b["foliage"]["foliage_block_data"]["farmer_reward_puzzle_hash"]
            self.blocks[h] = {
                "h": h,
                "kind": kind,
                "cls": cls,
                "ours": self.our_ph is not None and farmer_ph == self.our_ph,
                "ts": ftb["timestamp"] if ftb else None,
            }
        for h in sorted(self.blocks):
            if self.blocks[h]["kind"] not in self.kind_order:
                self.kind_order.append(self.blocks[h]["kind"])

    def _sync_blocks(self, peak: int) -> None:
        for h in [h for h in self.blocks if h > peak]:
            del self.blocks[h]
        # Re-read the tip so short reorgs are picked up, then backfill older blocks a chunk at a time.
        self._ingest_blocks(max(0, peak - 8), peak + 1)
        missing = [h for h in range(peak + 1) if h not in self.blocks]
        if missing:
            top = missing[-1]
            self._ingest_blocks(max(0, top - 199), top + 1)

    def _avg_block_time(self, peak: int) -> float:
        if peak < 600:
            return SECONDS_PER_BLOCK
        tx = [b for b in (self.blocks.get(h) for h in range(peak, peak - 600, -1)) if b and b["ts"]]
        if len(tx) < 2 or tx[0]["h"] == tx[-1]["h"]:
            return SECONDS_PER_BLOCK
        return (tx[0]["ts"] - tx[-1]["ts"]) / (tx[0]["h"] - tx[-1]["h"])

    def _measured_block_time(self, peak: int) -> float:
        """Observed seconds per block over up to the last 500 blocks; better than the flat default for a countdown."""
        tx = [b for b in (self.blocks.get(h) for h in range(peak, max(-1, peak - 500), -1)) if b and b["ts"]]
        if len(tx) < 10 or tx[0]["h"] - tx[-1]["h"] < 30:
            return SECONDS_PER_BLOCK
        return (tx[0]["ts"] - tx[-1]["ts"]) / (tx[0]["h"] - tx[-1]["h"])

    # ---- log ----------------------------------------------------------
    def _read_log(self) -> None:
        try:
            st = LOG_PATH.stat()
        except OSError:
            return
        if self.log_inode != st.st_ino or (self.log_offset or 0) > st.st_size:
            self.log_offset = max(0, st.st_size - 3_000_000) if self.log_inode is None else 0
            self.log_inode = st.st_ino
        with LOG_PATH.open("rb") as f:
            f.seek(self.log_offset or 0)
            chunk = f.read()
        cut = chunk.rfind(b"\n") + 1
        self.log_offset = (self.log_offset or 0) + cut
        for raw in chunk[:cut].decode("utf-8", "replace").splitlines():
            self._parse_line(raw)

    def _parse_line(self, line: str) -> None:
        if m := SP_RE.match(line):
            t, challenge, eligible, v1, v2, secs, plots = m.groups()
            if challenge:
                strength.harvester_sp(challenge, int(eligible))
            sp = {"t": log_time(t), "eligible": int(eligible), "v1": int(v1), "v2": int(v2), "proofs": int(v1) + int(v2), "s": float(secs), "plots": int(plots)}
            self.sps.append(sp)
            tot = self.sp_totals
            tot["count"] += 1
            tot["eligible"] += sp["eligible"]
            tot["passed"] += sp["eligible"] > 0
            tot["proofs"] += sp["proofs"]
            tot["v1"] += sp["v1"]
            tot["v2"] += sp["v2"]
            tot["max_s"] = max(tot["max_s"], sp["s"])
            tot["sum_s"] += sp["s"]
            if sp["proofs"]:
                found = [f"{sp['v1']} v1 proof" if sp["v1"] else "", f"{sp['v2']} v2 quality" if sp["v2"] else ""]
                self.pending_proofs.append({"t": sp["t"], "kind": "proof", "text": " and ".join(x for x in found if x)})
            return
        if m := FARMED_RE.match(line):
            t = log_time(m[1])
            self.farmed_times.append(t)
            # The proof behind a submitted block is shown as part of the win, not as its own event.
            self.pending_proofs = [p for p in self.pending_proofs if not (t - 5 <= p["t"] <= t)]
            return
        if (m := LEVEL_RE.match(line)) and not NOISE_RE.search(line):
            self.events.append({"t": log_time(m[1]), "kind": m[4].lower(), "text": f"{m[2]}: {m[5][:220]}"})

    def _flush_proofs(self, now: float) -> None:
        """Proofs that didn't lead to a submitted block within a few seconds get their own event."""
        keep = []
        for p in self.pending_proofs:
            (self.events.append(p) if now - p["t"] > 10 else keep.append(p))
        self.pending_proofs = keep

    def _block_time(self, h: int) -> float | None:
        """Only transaction blocks carry a timestamp; date any block by the first one at or after it."""
        return next((self.blocks[k]["ts"] for k in range(h, h + 20) if k in self.blocks and self.blocks[k]["ts"]), None)

    def _win_events(self, chain: list[dict]) -> list[dict]:
        """One event per block of ours in the chain. Timed by the node's submission when the log has one in the
        two minutes before the block's timestamp (each submission used once, oldest first, so back-to-back
        wins get their own), otherwise by the timestamp itself."""
        farmed = sorted(self.farmed_times)
        events = []
        for b in chain:
            if not b["ours"]:
                continue
            ts = self._block_time(b["h"])
            near = [t for t in farmed if ts is not None and ts - 120 <= t <= ts + 5]
            t = near[0] if near else ts
            if near:
                farmed.remove(t)
            events.append({"t": t, "kind": "win", "text": f"Won block {b['h']:,} with a {b['kind']} plot"})
        return events

    # ---- main loop ----------------------------------------------------
    def tick(self) -> None:
        if self.our_ph is None:
            targets = rpc("farmer", "get_reward_targets", {"search_for_private_key": False})
            if targets:
                self.our_ph = "0x" + decode_puzzle_hash(targets["farmer_target"]).hex()
                self.blocks.clear()  # re-ingest with ownership known

        now = time.time()
        state = rpc("full_node", "get_blockchain_state")
        conns = rpc("full_node", "get_connections")
        harvesters = rpc("farmer", "get_harvesters_summary")
        farmer_conns = rpc("farmer", "get_connections")
        plot_list = rpc("harvester", "get_plots")
        balance = rpc("wallet", "get_wallet_balance", {"wallet_id": 1})
        wsync = rpc("wallet", "get_sync_status")
        wheight = rpc("wallet", "get_height_info")
        self._read_log()
        self._flush_proofs(now)
        farmer_sps = rpc("farmer", "get_signage_points")

        node = None
        if state:
            bs = state["blockchain_state"]
            peak = bs["peak"]["height"] if bs["peak"] else 0
            self._sync_blocks(peak)
            node = {
                "height": peak,
                "synced": bs["sync"]["synced"],
                "sync_mode": bs["sync"]["sync_mode"],
                "sync_tip": bs["sync"]["sync_tip_height"],
                "space": bs["space"],
                "difficulty": bs["difficulty"],
                "ssi": bs["sub_slot_iters"],
                "mempool": bs["mempool_size"],
                "sp_index": bs["peak"]["signage_point_index"] if bs["peak"] else None,
                "avg_block_time": self._avg_block_time(peak),
            }

        farm = None
        if harvesters:
            hs = harvesters["harvesters"]
            farm = {
                "harvesters": len(hs),
                "plots": sum(h["plots"] for h in hs),
                "size": sum(h["total_plot_size"] for h in hs),
                "effective": sum(h["total_effective_plot_size"] for h in hs),
                "failed": sum(h["failed_to_open_filenames"] for h in hs),
                "no_key": sum(h["no_key_filenames"] for h in hs),
                "duplicates": sum(h["duplicates"] for h in hs),
            }
            by_version = {"v1": {"plots": 0, "size": 0}, "v2": {"plots": 0, "size": 0}}
            for plot in (plot_list or {}).get("plots", []):
                v = "v2" if plot["size"] & 0x80 else "v1"  # v2 plots report 0x80 | strength
                by_version[v]["plots"] += 1
                by_version[v]["size"] += plot["file_size"]
            farm["by_version"] = by_version if plot_list else None
            if plot_list and node:
                farm["expected_pass"] = expected_pass(plot_list.get("plots", []), node["height"])
            if node and node["space"]:
                share = farm["effective"] / node["space"]
                farm["share"] = share
                farm["etw"] = node["avg_block_time"] / share if share else None

        if node and now - self.last_history >= HISTORY_EVERY:
            self.last_history = now
            self.history.append({"t": now, "space": node["space"], "height": node["height"], "share": farm.get("share") if farm else None})

        peers = []
        for c in (conns or {}).get("connections", []):
            peers.append({
                "host": c["peer_host"],
                "port": c["peer_server_port"],
                "type": PEER_TYPES.get(c["type"], str(c["type"])),
                "height": c.get("peak_height"),
                "read": c["bytes_read"],
                "written": c["bytes_written"],
                "since": c["creation_time"],
            })

        chain = [self.blocks[h] for h in sorted(self.blocks)]
        peak_h = node["height"] if node else (chain[-1]["h"] if chain else -1)
        seq = "".join(self.blocks[h]["kind"][1] if h in self.blocks else "0" for h in range(peak_h + 1))
        ours_kind = {"v1": 0, "v2": 0}
        for b in chain:
            if b["ours"]:
                ours_kind[b["kind"]] += 1
        mix: dict[str, int] = {}
        for b in chain:
            mix[b["kind"]] = mix.get(b["kind"], 0) + 1
        ours = [b["h"] for b in chain if b["ours"]]
        last_win_t = self._block_time(ours[-1]) if ours else None

        wallet = None
        if balance:
            wb = balance["wallet_balance"]
            wallet = {
                "fingerprint": wb.get("fingerprint"),
                "confirmed": wb["confirmed_wallet_balance"],
                "unconfirmed": wb["unconfirmed_wallet_balance"],
                "spendable": wb["spendable_balance"],
                "coins": wb["unspent_coin_count"],
                "synced": bool(wsync and wsync["synced"]),
                "syncing": bool(wsync and wsync["syncing"]),
                "height": wheight["height"] if wheight else None,
            }

        fork = None
        if node:
            bt = self._measured_block_time(node["height"])
            # Anchor on the newest transaction block's timestamp, not on "now": a now-based ETA slides
            # forward on every poll between blocks, so a client countdown never actually counts down.
            anchor_h, anchor_t = node["height"], now
            for h in range(node["height"], max(-1, node["height"] - 50), -1):
                b = self.blocks.get(h)
                if b and b["ts"]:
                    anchor_h, anchor_t = h, b["ts"]
                    break
            fork = {
                "height": FORK_HEIGHT,
                "v1_cutoff": V1_CUTOFF,
                "phase_out_epochs": (V1_CUTOFF - FORK_HEIGHT) // int(CONSTANTS.EPOCH_BLOCKS),
                "block_time": bt,
                "eta": anchor_t + (FORK_HEIGHT - anchor_h) * bt,
                "cutoff_eta": anchor_t + (V1_CUTOFF - anchor_h) * bt,
            }

        # Filter results per strength for any new signage points, against the newest transaction block.
        stat_plots = plots_for_stats(plot_list)
        last_tx = next((h for h in range(node["height"], max(-1, node["height"] - 50), -1) if self.blocks.get(h, {}).get("ts")), None) if node else None
        if node and farmer_sps and plot_list and last_tx is not None:
            strength.update(farmer_sps["signage_points"], stat_plots, node["height"], last_tx)
        strength_snapshot = strength.snapshot(stat_plots, chain, last_tx)

        tot = self.sp_totals
        snapshot = {
            "fork": fork,
            "ready": True,
            "now": now,
            "network": CONFIG["selected_network"],
            "services": {
                "node": state is not None,
                "farmer": harvesters is not None,
                "solver": farmer_conns is not None and any(c["type"] == 8 for c in farmer_conns["connections"]),
                "wallet": balance is not None,
            },
            "node": node,
            "farm": farm,
            "wallet": wallet,
            "peers": peers,
            "kinds": self.kind_order,
            "chain": {"count": len(chain), "mix": mix, "ours": ours, "ours_by_kind": ours_kind, "seq": seq, "last_win_t": last_win_t},
            "history": list(self.history),
            "sps": list(self.sps)[-240:],
            "sp_totals": {**tot, "avg_s": tot["sum_s"] / tot["count"] if tot["count"] else 0.0},
            "events": sorted(list(self.events) + self._win_events(chain), key=lambda e: e["t"] or 0, reverse=True)[:40],
            "started": self.started,
            "strength": strength_snapshot,
            "plotting": plotting_status({Path(p["filename"]).name for p in plot_list["plots"]} if plot_list else None),
        }
        with self.lock:
            self.snapshot = snapshot

    def run(self) -> None:
        while True:
            try:
                self.tick()
            except Exception as e:  # keep the dashboard alive; surface the failure in the feed
                self.events.append({"t": time.time(), "kind": "error", "text": f"dashboard collector: {e!r}"})
            time.sleep(POLL_SECONDS)


collector = Collector()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/api/state":
            with collector.lock:
                body = json.dumps(collector.snapshot).encode()
            ctype = "application/json"
        elif self.path in ("/", "/index.html"):
            body = (HERE / "index.html").read_bytes()
            ctype = "text/html; charset=utf-8"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args: object) -> None:
        pass


if __name__ == "__main__":
    try:
        httpd = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    except OSError as e:
        raise SystemExit(f"Can't listen on port {PORT} ({e.strerror}). Pick another one with --port.") from None
    threading.Thread(target=collector.run, daemon=True).start()
    url = f"http://127.0.0.1:{PORT}"
    print(f"Dashboard for {CONFIG['selected_network']} running at {url}  (Ctrl-C to stop)", flush=True)
    if ARGS.open:
        import webbrowser

        webbrowser.open(url)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
