"""General-layer regression gate (相位 2 地基).

When the general layer is retrained (相位 2: modern-corpus refresh to fix stale
picks like dazi->大字), this gate decides — fully automatically, no human in the
loop — whether the new artifacts may replace the old ones:

    A. control set (public PKU held-out) top-1 must NOT drop  (control_Δ >= 0)
    B. target  set (stale cases to fix)  top-1 must IMPROVE   (target_Δ  > 0)
    exit 0 iff (A and B) else 1

The control set is frozen into gate/control.tsv (public SIGHAN data, committable,
reproducible regardless of the gitignored corpus). The target set is a small
hand-maintained gate/target.tsv (keys<TAB>gold); both are general-layer only —
the gate always decodes with --no-user-data, the personal layer is measured
elsewhere. Per-case flips are written to the report for debugging a FAIL; they
are never an approval step.

Usage:
    # (A) freeze the control set once (needs data/general/pku_training.utf8):
    .venv\\Scripts\\python.exe gate.py --rebuild-control [--holdout 400] [--max 200] [--seed 29]

    # (B) run the gate (needs only mochi-brain.exe + the two tsv files):
    .venv\\Scripts\\python.exe gate.py <old_artifacts_dir> [new_artifacts_dir]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

BRAIN = ROOT.parent.parent / "src" / "brain" / "target" / "release" / "mochi-brain.exe"
GATE_DIR = ROOT / "gate"
DEFAULT_NEW = ROOT.parent.parent / "artifacts" / "v0"

# CreateProcess command lines are capped (~32k chars); keys are short pinyin
# strings, so a generous chunk stays well under the limit with margin.
DECODE_CHUNK = 800


# --------------------------------------------------------------- decode helpers
def decode_top1(artifacts: Path, keys: list[str]) -> dict[str, str]:
    """key -> top-1 text, general layer only. Decodes unique keys in chunks."""
    uniq = list(dict.fromkeys(keys))
    top1: dict[str, str] = {}
    for i in range(0, len(uniq), DECODE_CHUNK):
        chunk = uniq[i : i + DECODE_CHUNK]
        proc = subprocess.run(
            [str(BRAIN), "--artifacts", str(artifacts), "--no-user-data",
             "--topn", "1", "--decode", *chunk],
            capture_output=True, text=True, encoding="utf-8",
        )
        if proc.returncode != 0:
            sys.exit(f"brain failed to decode against {artifacts} "
                     f"(exit {proc.returncode}): {proc.stderr.strip().splitlines()[-1] if proc.stderr.strip() else 'no stderr'}")
        for line in proc.stdout.splitlines():
            cols = line.split("\t")
            # main.rs decode line: keys \t #N \t text \t comment \t preedit \t quality \t us
            if len(cols) >= 3 and cols[1] == "#1":
                top1[cols[0]] = cols[2]
    return top1


def load_cases(path: Path) -> list[tuple[str, str]]:
    """Read a `keys<TAB>gold` tsv (blank lines / # comments ignored)."""
    items: list[tuple[str, str]] = []
    if not path.exists():
        return items
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 2 and parts[0] and parts[1]:
            items.append((parts[0], parts[1]))
    return items


def score(items: list[tuple[str, str]], top1: dict[str, str]) -> list[bool]:
    """Per-item top-1 exact match (got == gold)."""
    return [top1.get(keys, "") == gold for keys, gold in items]


# --------------------------------------------------------------- (A) freeze ctrl
def rebuild_control(holdout: int, max_items: int, seed: int) -> None:
    import random

    from pipeline.corpus import extract_clauses, iter_general_sentences
    from pipeline.pinyinize import make_eval_item

    pku_path = ROOT / "data" / "general" / "pku_training.utf8"
    if not pku_path.exists():
        sys.exit(f"need {pku_path} (run pipeline.prepare_data first)")

    # exactly run_experiment.py methodology: the tail `holdout` PKU sentences,
    # then clause-split + keystream, ascii pinyin keys of sane length.
    pku = list(iter_general_sentences(pku_path))
    control_sents = pku[-holdout:] if holdout else []
    items = []
    for sent in control_sents:
        for clause in extract_clauses("".join(sent)):
            it = make_eval_item(clause, source="pku_control")
            if it.keys and it.keys.isascii() and 4 <= len(it.keys) <= 30:
                items.append(it)
    random.Random(seed).shuffle(items)
    items = items[:max_items]

    GATE_DIR.mkdir(exist_ok=True)
    out = GATE_DIR / "control.tsv"
    with out.open("w", encoding="utf-8", newline="\n") as f:
        f.write("# Mochi general-layer control set (public PKU held-out; do not hand-edit).\n")
        f.write(f"# rebuilt via: gate.py --rebuild-control --holdout {holdout} --max {max_items} --seed {seed}\n")
        f.write("# columns: keys<TAB>gold\n")
        for it in items:
            f.write(f"{it.keys}\t{it.target}\n")
    print(f"wrote {out} ({len(items)} clauses; holdout={holdout} seed={seed})")


# --------------------------------------------------------------- (B) run gate
def run_gate(old: Path, new: Path, control_path: Path, target_path: Path,
             report_path: Path) -> int:
    control = load_cases(control_path)
    target = load_cases(target_path)
    if not control:
        sys.exit(f"empty control set {control_path} — run gate.py --rebuild-control first")
    if not target:
        sys.exit(f"empty target set {target_path} — add stale cases (keys<TAB>gold)")

    all_keys = [k for k, _ in control] + [k for k, _ in target]
    old_top1 = decode_top1(old, all_keys)
    new_top1 = decode_top1(new, all_keys)

    def summarize(items, label):
        o = score(items, old_top1)
        n = score(items, new_top1)
        flips = []
        for (keys, gold), oh, nh in zip(items, o, n):
            if oh != nh:
                flips.append({
                    "keys": keys, "gold": gold,
                    "old": old_top1.get(keys, ""), "new": new_top1.get(keys, ""),
                    "result": "gained" if nh and not oh else "lost",
                })
        return {
            "label": label, "n": len(items),
            "old_hits": sum(o), "new_hits": sum(n),
            "delta": sum(n) - sum(o), "flips": flips,
        }

    ctrl = summarize(control, "control")
    tgt = summarize(target, "target")

    cond_a = ctrl["delta"] >= 0          # control must not regress
    cond_b = tgt["delta"] > 0            # target must improve
    passed = cond_a and cond_b

    # ---- report (debugging aid for a FAIL; never an approval step) ----
    GATE_DIR.mkdir(exist_ok=True)
    payload = {
        "ts": datetime.now().isoformat(timespec="seconds"),
        "old": str(old), "new": str(new),
        "pass": passed, "cond_control_no_drop": cond_a, "cond_target_up": cond_b,
        "control": ctrl, "target": tgt,
    }
    report_path.with_suffix(".json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    _write_md(report_path, payload)

    # ---- stdout summary ----
    def pct(h, n):
        return f"{h}/{n} = {h / n:.1%}" if n else "0/0"
    print(f"# gate  old={old}  new={new}")
    print(f"control  old {pct(ctrl['old_hits'], ctrl['n'])}  ->  new {pct(ctrl['new_hits'], ctrl['n'])}  Δ={ctrl['delta']:+d}  [{'OK' if cond_a else 'FAIL'} no-drop]")
    print(f"target   old {pct(tgt['old_hits'], tgt['n'])}  ->  new {pct(tgt['new_hits'], tgt['n'])}  Δ={tgt['delta']:+d}  [{'OK' if cond_b else 'FAIL'} improve]")
    lost = [f for f in ctrl["flips"] if f["result"] == "lost"]
    if lost:
        print(f"control regressions ({len(lost)}, see report):")
        for f in lost[:10]:
            print(f"  - {f['keys']}\tgold={f['gold']}\told={f['old']}\tnew={f['new']}")
    print(f"report: {report_path}")
    print("PASS" if passed else "FAIL")
    return 0 if passed else 1


def _write_md(path: Path, p: dict) -> None:
    lines = [f"# 门禁报告 {p['ts']}", "",
             f"- old: `{p['old']}`", f"- new: `{p['new']}`",
             f"- **裁决: {'PASS' if p['pass'] else 'FAIL'}**"
             f"（对照不退={p['cond_control_no_drop']} / 目标提升={p['cond_target_up']}）", ""]
    for sec in (p["control"], p["target"]):
        lines.append(f"## {sec['label']}  old {sec['old_hits']}/{sec['n']} → new {sec['new_hits']}/{sec['n']}  Δ={sec['delta']:+d}")
        if sec["flips"]:
            lines.append("| 结果 | keys | gold | old | new |")
            lines.append("|---|---|---|---|---|")
            for f in sec["flips"]:
                lines.append(f"| {f['result']} | `{f['keys']}` | {f['gold']} | {f['old']} | {f['new']} |")
        else:
            lines.append("（无翻转）")
        lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser(description="Mochi general-layer regression gate")
    ap.add_argument("old", nargs="?", help="old artifacts dir (baseline)")
    ap.add_argument("new", nargs="?", help="new artifacts dir (default: artifacts/v0)")
    ap.add_argument("--rebuild-control", action="store_true",
                    help="freeze gate/control.tsv from PKU held-out, then exit")
    ap.add_argument("--holdout", type=int, default=400)
    ap.add_argument("--max", type=int, default=200)
    ap.add_argument("--seed", type=int, default=29)
    ap.add_argument("--control", default=str(GATE_DIR / "control.tsv"))
    ap.add_argument("--target", default=str(GATE_DIR / "target.tsv"))
    ap.add_argument("--report", default=str(GATE_DIR / "last-report.md"))
    args = ap.parse_args()

    if args.rebuild_control:
        rebuild_control(args.holdout, args.max, args.seed)
        return
    if not args.old:
        ap.error("need <old_artifacts_dir> (or --rebuild-control)")
    new = Path(args.new) if args.new else DEFAULT_NEW
    sys.exit(run_gate(Path(args.old), new, Path(args.control),
                      Path(args.target), Path(args.report)))


if __name__ == "__main__":
    main()
