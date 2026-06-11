"""A/B regression: decode corpus clauses against two artifact builds.

Used when the general layer changes (e.g. the essay base, M2-4): the new
build must not lose top-1 accuracy on real clauses. Both runs are general
layer only (--no-user-data) — personal-layer gains are measured elsewhere.

    .\.venv\Scripts\python.exe eval_artifacts_ab.py <old_artifacts_dir> [new_dir] [n]

Prints aggregate hit rates plus the flipped cases (gained/lost). Clause
texts are private corpus content: console only, never into reports.
"""

from __future__ import annotations

import random
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from pipeline.corpus import extract_clauses, load_personal_documents
from pipeline.pinyinize import make_eval_item

BRAIN = ROOT.parent.parent / "src" / "brain" / "target" / "release" / "mochi-brain.exe"


def sample_items(n: int) -> list:
    docs = load_personal_documents(ROOT / "data" / "personal")
    clauses = [c for _, text in docs for c in extract_clauses(text)]
    rng = random.Random(7)
    rng.shuffle(clauses)
    items = []
    for c in clauses:
        item = make_eval_item(c)
        if item.keys and item.keys.isascii() and 4 <= len(item.keys) <= 30:
            items.append(item)
        if len(items) >= n:
            break
    return items


def decode_all(artifacts: Path, keys: list[str]) -> dict[str, str]:
    out = subprocess.run(
        [str(BRAIN), "--artifacts", str(artifacts), "--no-user-data",
         "--topn", "1", "--decode", *keys],
        capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    top1 = {}
    for line in out.splitlines():
        cols = line.split("\t")
        if len(cols) >= 3 and cols[1] == "#1":
            top1[cols[0]] = cols[2]
    return top1


def main() -> None:
    old_dir = Path(sys.argv[1])
    new_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT.parent.parent / "artifacts" / "v0"
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 80
    items = sample_items(n)
    keys = [i.keys for i in items]
    print(f"# {len(items)} clauses; old={old_dir} new={new_dir}")
    old = decode_all(old_dir, keys)
    new = decode_all(new_dir, keys)
    hits_old = hits_new = 0
    gained, lost = [], []
    for it in items:
        o = old.get(it.keys, "") == it.target
        nw = new.get(it.keys, "") == it.target
        hits_old += o
        hits_new += nw
        if nw and not o:
            gained.append(it)
        if o and not nw:
            lost.append(it)
    print(f"old top-1: {hits_old}/{len(items)} = {hits_old / len(items):.1%}")
    print(f"new top-1: {hits_new}/{len(items)} = {hits_new / len(items):.1%}")
    print(f"gained {len(gained)}, lost {len(lost)}")
    for it in gained:
        print(f"  + {it.keys}\t{it.target}\t(old: {old.get(it.keys)})")
    for it in lost:
        print(f"  - {it.keys}\t{it.target}\t(new: {new.get(it.keys)})")


if __name__ == "__main__":
    main()
