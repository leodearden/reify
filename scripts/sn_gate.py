#!/usr/bin/env python3
"""
sn_gate.py — A′ s(N) gate: estimate s(N) from reify main history and apply the
deterministic go/no-go decision for the coupling-tolerant train former (task 4455).

PART 1 — estimate s(N):
  From reify `git log --first-parent main --pretty=format:%h|%cI|%s`, identify
  clusters of near-simultaneous merges (the candidates a former would batch) and
  estimate the combined-verify success rate s(N) for N=2 and N=3 via the proxy:
  rate(cluster followed by a fix-forward) ≈ 1 − s(N).

PART 2 — apply the deterministic decision (D7.3):
  break-even 1/N; marginal half-width = 0.2·(1/N).
  deciding N = largest N∈{3,2} with s(N) > 1/N.
  GO   → s ≥ 1/N + 0.2/N AND sample ≥ 10 AND not ambiguous.
  NO-GO → no N clears AND s(2) ≤ 0.40 AND sample(2) ≥ 10 AND not ambiguous.
  MARGINAL → anything else.

Usage:
    python3 scripts/sn_gate.py [--from-git | --git-log-file FILE]
                               [--window SECS] [--lookahead SECS]
                               [--runs-db PATH]
                               [--json] [--markdown]
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class CommitInfo:
    sha: str
    timestamp: datetime  # tz-aware
    subject: str
    is_merge: bool
    task_id: Optional[str]  # e.g. "4455" for "Merge task/4455 into main"


@dataclass
class Cluster:
    merges: List[CommitInfo] = field(default_factory=list)

    @property
    def task_ids(self) -> List[str]:
        return [m.task_id for m in self.merges if m.task_id]

    @property
    def last_ts(self) -> Optional[datetime]:
        return self.merges[-1].timestamp if self.merges else None

    @property
    def size(self) -> int:
        return len(self.merges)


@dataclass
class EstimateResult:
    n: int
    s_point: float
    sample_size: int
    ambiguous_frac: float  # weak attributions / followed (0 when none followed)


@dataclass
class DecideResult:
    classification: str   # "GO" | "NO-GO" | "MARGINAL"
    chosen_N: Optional[int]
    deciding_N: Optional[int]
    reason: str
    per_n: Dict[int, Dict[str, Any]] = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Core pure functions (tested by test_sn_gate.py)
# ---------------------------------------------------------------------------

def parse_git_log_line(line: str) -> Optional[CommitInfo]:
    """Parse a single `git log --pretty=format:%h|%cI|%s` line.

    Returns CommitInfo or None if the line is malformed / empty.
    Merges are identified by subject matching 'Merge task/(\\d+) into main'.
    """
    line = line.strip()
    if not line:
        return None
    parts = line.split("|", 2)
    if len(parts) < 3:
        return None
    sha, iso_ts, subject = parts[0], parts[1], parts[2]
    try:
        ts = datetime.fromisoformat(iso_ts)
        if ts.tzinfo is None:
            ts = ts.replace(tzinfo=timezone.utc)
    except ValueError:
        return None
    m = re.match(r"Merge task/(\d+)", subject)
    is_merge = m is not None
    task_id = m.group(1) if m else None
    return CommitInfo(sha=sha, timestamp=ts, subject=subject,
                      is_merge=is_merge, task_id=task_id)


def cluster_merges(merges: List[CommitInfo],
                   window_seconds: float = 180.0) -> List[Cluster]:
    """Group merges into clusters using greedy single-linkage.

    A merge joins the current cluster iff its gap from the previous merge is
    ≤ window_seconds. Sorted ascending by timestamp first.
    Returns a list of Cluster objects (singletons included).
    """
    if not merges:
        return []
    sorted_merges = sorted(merges, key=lambda c: c.timestamp)
    clusters: List[Cluster] = []
    current = Cluster([sorted_merges[0]])
    for commit in sorted_merges[1:]:
        gap = (commit.timestamp - current.merges[-1].timestamp).total_seconds()
        if gap <= window_seconds:
            current.merges.append(commit)
        else:
            clusters.append(current)
            current = Cluster([commit])
    clusters.append(current)
    return clusters


def is_fix_forward(subject: str) -> bool:
    """Return True if subject looks like a fix-forward / revert commit."""
    return bool(re.match(
        r"^(fix[\(:/-]|fix forward|revert|hotfix)",
        subject.strip(),
        re.IGNORECASE,
    ))


def attribute_fix_forward(
    cluster: Cluster,
    following_commits: List[CommitInfo],
    lookahead_seconds: float = 86400.0,
) -> Tuple[bool, bool]:
    """Determine whether a fix-forward follows a cluster, and if so how strongly.

    Args:
        cluster: The merge cluster to test.
        following_commits: All commits (merge + non-merge) ordered chronologically
            AFTER the cluster's last merge.
        lookahead_seconds: Maximum seconds after cluster's last merge to search.

    Returns:
        (followed, strong):
          followed — a fix-forward commit appears within lookahead after the
                     cluster's last merge timestamp.
          strong   — the fix-forward commit references a task_id that is a
                     member of this cluster (strong attribution);
                     False → weak (ambiguous attribution).
    """
    if not cluster.last_ts:
        return (False, False)
    cluster_task_ids = set(cluster.task_ids)
    deadline = cluster.last_ts.timestamp() + lookahead_seconds

    for commit in following_commits:
        if commit.timestamp.timestamp() > deadline:
            break
        if not is_fix_forward(commit.subject):
            continue
        # It's a fix-forward — now check attribution strength.
        # Strong: subject references one of the cluster's task IDs.
        strong = False
        if cluster_task_ids:
            for tid in cluster_task_ids:
                if re.search(
                    r"(task[/\s#]+{tid}|\(task\s+{tid}\)|revert.*task.*{tid})".format(tid=re.escape(tid)),
                    commit.subject,
                    re.IGNORECASE,
                ):
                    strong = True
                    break
        return (True, strong)
    return (False, False)


def estimate_s(
    clusters: List[Cluster],
    following_index: Dict[int, List[CommitInfo]],
    n: int,
    lookahead: float = 86400.0,
) -> EstimateResult:
    """Estimate s(N) for the given N.

    sample_size = number of clusters with ≥ n members.
    s_point = 1 − (clusters followed by fix-forward / sample_size).
    ambiguous_frac = weak-attributions / followed (0 when none followed).

    Args:
        clusters: All clusters (from cluster_merges).
        following_index: Maps cluster index (in `clusters`) to the list of
            commits that follow it (for attribute_fix_forward).
        n: Minimum cluster size for inclusion in the sample.
        lookahead: Passed through to attribute_fix_forward.
    """
    eligible = [(i, c) for i, c in enumerate(clusters) if c.size >= n]
    sample_size = len(eligible)
    if sample_size == 0:
        return EstimateResult(n=n, s_point=float("nan"), sample_size=0, ambiguous_frac=0.0)

    followed = 0
    weak = 0
    for i, cluster in eligible:
        fc = following_index.get(i, [])
        did_follow, strong = attribute_fix_forward(cluster, fc, lookahead)
        if did_follow:
            followed += 1
            if not strong:
                weak += 1

    failure_rate = followed / sample_size
    s_point = 1.0 - failure_rate
    ambiguous_frac = (weak / followed) if followed > 0 else 0.0
    return EstimateResult(n=n, s_point=s_point, sample_size=sample_size,
                          ambiguous_frac=ambiguous_frac)


def decide(
    s2: float,
    n2: int,
    s3: float,
    n3: int,
    amb2: float,
    amb3: float,
    thin: int = 10,
    margin_frac: float = 0.2,
) -> DecideResult:
    """Apply the deterministic go/no-go decision rule (D7.3).

    break-even: 1/N; marginal band half-width: margin_frac/N.
    N=2 band: (0.40, 0.60); N=3 band: (0.2667, 0.40).
    deciding N = largest N∈{3,2} with s(N) > 1/N.
    GO if deciding N: s ≥ 1/N + margin_frac/N AND sample ≥ thin AND not ambiguous.
    NO-GO if no N clears: s(2) ≤ 1/2 − margin_frac/2 AND n2 ≥ thin AND not amb2.
    MARGINAL otherwise.
    """
    import math

    def per_n_info(n: int, s: float, ns: int, amb: float) -> Dict[str, Any]:
        if math.isnan(s):
            return dict(s=s, n=ns, clears=False, margin=False, in_band=False,
                        thin=ns < thin, ambiguous=amb > 0)
        break_even = 1.0 / n
        upper = break_even + margin_frac / n
        lower = break_even - margin_frac / n
        clears = s > break_even
        margin = s >= upper
        in_band = lower < s <= upper
        return dict(s=s, n=ns, clears=clears, margin=margin, in_band=in_band,
                    thin=ns < thin, ambiguous=amb > 0)

    info2 = per_n_info(2, s2, n2, amb2)
    info3 = per_n_info(3, s3, n3, amb3)
    per_n = {2: info2, 3: info3}

    # deciding N = largest clearing N
    deciding_N: Optional[int] = None
    for n_cand in [3, 2]:
        if per_n[n_cand]["clears"]:
            deciding_N = n_cand
            break

    if deciding_N is not None:
        info = per_n[deciding_N]
        if info["margin"] and not info["thin"] and not info["ambiguous"]:
            return DecideResult(
                classification="GO",
                chosen_N=deciding_N,
                deciding_N=deciding_N,
                reason=f"s({deciding_N})={per_n[deciding_N]['s']:.3f} clears 1/{deciding_N} with margin, n={per_n[deciding_N]['n']} ≥ {thin}, not ambiguous",
                per_n=per_n,
            )
        else:
            reasons = []
            if info["in_band"]:
                reasons.append(f"s({deciding_N}) in marginal band")
            elif not info["margin"]:
                reasons.append(f"s({deciding_N}) clears but below upper margin")
            if info["thin"]:
                reasons.append(f"n({deciding_N})={info['n']} < {thin} (thin sample)")
            if info["ambiguous"]:
                reasons.append(f"ambiguous fix-forward attribution at N={deciding_N}")
            return DecideResult(
                classification="MARGINAL",
                chosen_N=None,
                deciding_N=deciding_N,
                reason="; ".join(reasons) or "marginal",
                per_n=per_n,
            )
    else:
        # No N clears break-even
        if (not info2["thin"] and not info2["ambiguous"] and
                s2 <= (0.5 - margin_frac / 2)):
            return DecideResult(
                classification="NO-GO",
                chosen_N=None,
                deciding_N=None,
                reason=f"s(2)={s2:.3f} ≤ {0.5 - margin_frac/2:.3f} (lower band), n={n2} ≥ {thin}, not ambiguous; no N clears 1/N",
                per_n=per_n,
            )
        else:
            reasons = []
            if not info2["clears"]:
                if info2["in_band"]:
                    reasons.append("s(2) in marginal band (no N clears)")
                else:
                    reasons.append("s(2) does not clear 1/2 but not comfortably below 0.40")
            if info2["thin"]:
                reasons.append(f"n(2)={info2['n']} < {thin} (thin sample)")
            if info2["ambiguous"]:
                reasons.append("ambiguous fix-forward attribution at N=2")
            return DecideResult(
                classification="MARGINAL",
                chosen_N=None,
                deciding_N=None,
                reason="; ".join(reasons) or "marginal (no N clears, conditions unclear)",
                per_n=per_n,
            )


def build_report(
    result2: EstimateResult,
    result3: EstimateResult,
    decision: DecideResult,
    window: float,
    lookahead: float,
) -> Tuple[Dict[str, Any], str]:
    """Build machine-readable JSON summary and a markdown report body.

    Returns (summary_dict, markdown_text).
    """
    recommended_actions = {
        "GO": "flip 1705-1708 pending",
        "NO-GO": "cancel 1705-1708 + info-escalate",
        "MARGINAL": "leave deferred + escalate",
    }
    summary: Dict[str, Any] = {
        "s2": result2.s_point,
        "n2": result2.sample_size,
        "s3": result3.s_point,
        "n3": result3.sample_size,
        "ambiguous": result2.ambiguous_frac > 0 or result3.ambiguous_frac > 0,
        "ambiguous2": result2.ambiguous_frac,
        "ambiguous3": result3.ambiguous_frac,
        "classification": decision.classification,
        "chosen_N": decision.chosen_N,
        "deciding_N": decision.deciding_N,
        "recommended_action": recommended_actions.get(decision.classification, "unknown"),
    }

    def fmt_s(s: float) -> str:
        import math
        return "N/A (no sample)" if math.isnan(s) else f"{s:.3f}"

    md = f"""\
# A′ s(N) Gate — Train Former Decision (task 4455)

## Methodology

- **Proxy**: rate(cluster followed by fix-forward within `{lookahead:.0f}s`) ≈ 1 − s(N)
- **Cluster window**: `{window:.0f}s` (merges within this gap from the previous merge join one cluster)
- **Break-even**: 1/N; **marginal band**: [1/N − 0.2/N, 1/N + 0.2/N]
  - N=2: (0.40, 0.60) | N=3: (0.267, 0.40)
- **Thin sample**: < 10 N-clusters
- **Attribution**: strong (fix references a cluster task_id) vs weak (no explicit reference)

## Estimates

| N | s(N) | Sample size | Ambiguous frac |
|---|------|-------------|----------------|
| 2 | {fmt_s(result2.s_point)} | {result2.sample_size} | {result2.ambiguous_frac:.3f} |
| 3 | {fmt_s(result3.s_point)} | {result3.sample_size} | {result3.ambiguous_frac:.3f} |

## Decision

**Classification**: {decision.classification}

**Reason**: {decision.reason}

**Chosen N** (merge.train_max_members): {decision.chosen_N if decision.chosen_N is not None else 'N/A'}

**Recommended action**: {summary['recommended_action']}
"""
    return summary, md


def summarize_runs_db(
    conn: sqlite3.Connection,
    cluster_task_ids: List[str],
) -> Optional[Dict[str, Any]]:
    """Summarize corroborating signals from a dark-factory runs.db.

    Counts cluster members with verify_attempts > 1 or post-merge re-verify
    events. Returns a dict or None if tables/columns are unavailable.
    """
    try:
        cursor = conn.cursor()
        # Check tables exist
        tables = {row[0] for row in cursor.execute(
            "SELECT name FROM sqlite_master WHERE type='table'"
        )}
        if "task_results" not in tables and "events" not in tables:
            return None

        result: Dict[str, Any] = {}

        if "task_results" in tables and cluster_task_ids:
            placeholders = ",".join("?" * len(cluster_task_ids))
            try:
                rows = cursor.execute(
                    f"SELECT task_id, verify_attempts FROM task_results "
                    f"WHERE task_id IN ({placeholders})",
                    cluster_task_ids,
                ).fetchall()
                result["members_with_extra_verify_attempts"] = [
                    r[0] for r in rows if r[1] is not None and r[1] > 1
                ]
            except sqlite3.OperationalError:
                pass  # column missing — tolerate

        if "events" in tables and cluster_task_ids:
            placeholders = ",".join("?" * len(cluster_task_ids))
            try:
                rows = cursor.execute(
                    f"SELECT DISTINCT task_id FROM events "
                    f"WHERE task_id IN ({placeholders}) "
                    f"  AND event_type = 're-verify'",
                    cluster_task_ids,
                ).fetchall()
                result["members_with_reverify_event"] = [r[0] for r in rows]
            except sqlite3.OperationalError:
                pass

        return result if result else None
    except Exception:
        return None


# ---------------------------------------------------------------------------
# I/O helpers
# ---------------------------------------------------------------------------

def _build_following_index(
    clusters: List[Cluster],
    all_commits: List[CommitInfo],
) -> Dict[int, List[CommitInfo]]:
    """Map each cluster index to commits appearing AFTER that cluster ends."""
    # all_commits sorted ascending by timestamp
    following: Dict[int, List[CommitInfo]] = {}
    for idx, cluster in enumerate(clusters):
        if cluster.last_ts is None:
            following[idx] = []
            continue
        last_ts = cluster.last_ts
        following[idx] = [c for c in all_commits if c.timestamp > last_ts]
    return following


def _ingest_lines(lines: List[str]) -> List[CommitInfo]:
    commits = []
    for line in lines:
        info = parse_git_log_line(line)
        if info is not None:
            commits.append(info)
    return commits


# ---------------------------------------------------------------------------
# CLI entry-point
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(
        description=("Estimate s(N) from reify main history and apply the "
                     "A′ go/no-go decision (task 4455)."),
    )
    src = parser.add_mutually_exclusive_group()
    src.add_argument(
        "--from-git", action="store_true",
        help="Run `git log --first-parent main --pretty=format:%%h|%%cI|%%s` live.",
    )
    src.add_argument(
        "--git-log-file", metavar="FILE",
        help="Read pre-captured git log output from FILE (- for stdin).",
    )
    parser.add_argument(
        "--window", type=float, default=300.0, metavar="SECS",
        help="Cluster window in seconds (default: 300).",
    )
    parser.add_argument(
        "--lookahead", type=float, default=86400.0, metavar="SECS",
        help="Look-ahead window for fix-forward detection in seconds (default: 86400).",
    )
    parser.add_argument(
        "--runs-db", metavar="PATH",
        help="Optional path to dark-factory runs.db for corroboration.",
    )
    parser.add_argument(
        "--json", action="store_true", dest="emit_json",
        help="Emit machine-readable JSON summary to stdout.",
    )
    parser.add_argument(
        "--markdown", action="store_true", dest="emit_markdown",
        help="Emit markdown report to stdout.",
    )
    args = parser.parse_args()

    # ── Ingest ──────────────────────────────────────────────────────────────
    if args.from_git:
        proc = subprocess.run(
            ["git", "log", "--first-parent", "main",
             "--pretty=format:%h|%cI|%s"],
            capture_output=True, text=True, check=True,
        )
        lines = proc.stdout.splitlines()
    elif args.git_log_file:
        if args.git_log_file == "-":
            lines = sys.stdin.read().splitlines()
        else:
            lines = Path(args.git_log_file).read_text().splitlines()
    else:
        parser.print_help(sys.stderr)
        sys.stderr.write("\nError: one of --from-git or --git-log-file is required.\n")
        sys.exit(1)

    all_commits = _ingest_lines(lines)
    merges = [c for c in all_commits if c.is_merge]

    clusters = cluster_merges(merges, window_seconds=args.window)
    following_index = _build_following_index(clusters, all_commits)

    result2 = estimate_s(clusters, following_index, n=2, lookahead=args.lookahead)
    result3 = estimate_s(clusters, following_index, n=3, lookahead=args.lookahead)

    decision = decide(
        s2=result2.s_point, n2=result2.sample_size,
        s3=result3.s_point, n3=result3.sample_size,
        amb2=result2.ambiguous_frac, amb3=result3.ambiguous_frac,
    )

    summary, md_text = build_report(result2, result3, decision,
                                    window=args.window, lookahead=args.lookahead)

    # ── Optional runs.db corroboration ─────────────────────────────────────
    if args.runs_db:
        cluster_tids = [tid for c in clusters for tid in c.task_ids]
        try:
            conn = sqlite3.connect(args.runs_db)
            corroboration = summarize_runs_db(conn, cluster_tids)
            conn.close()
            if corroboration:
                summary["runs_db_corroboration"] = corroboration
        except Exception as exc:
            sys.stderr.write(f"Warning: could not open runs.db: {exc}\n")

    # ── Output ──────────────────────────────────────────────────────────────
    if args.emit_json:
        print(json.dumps(summary, indent=2, default=str))
    if args.emit_markdown:
        print(md_text)
    if not args.emit_json and not args.emit_markdown:
        # Default: print a brief human summary
        import math
        s2_str = "N/A" if math.isnan(result2.s_point) else f"{result2.s_point:.3f}"
        s3_str = "N/A" if math.isnan(result3.s_point) else f"{result3.s_point:.3f}"
        print(f"s(2)={s2_str} n={result2.sample_size}  "
              f"s(3)={s3_str} n={result3.sample_size}")
        print(f"Classification: {decision.classification}")
        if decision.chosen_N:
            print(f"Chosen N (merge.train_max_members): {decision.chosen_N}")
        print(f"Reason: {decision.reason}")
        print(f"Recommended action: {summary['recommended_action']}")


if __name__ == "__main__":
    main()
