"""Export the personal layer as personal-artifacts-v0 (cold-start memory).

Builds the personal layer from the user's document corpus with the exact
experiment-001 recipe (jieba tokenization, per-document time decay, full-text
English term harvesting) and writes one TSV the brain loads at startup:

    user_data/personal.tsv
        lex<TAB>word<TAB>decayed_count        lexicon (en lowercase / hanzi)
        case<TAB>lower<TAB>Surface            preferred casing ("resnet"->"ResNet")
        zhword<TAB>word<TAB>spaced syllables  dict-OOV Chinese words (lattice arcs)
        1|2|3<TAB>tokens<TAB>count            personal trigram LM (weighted)

This file is PRIVATE user data (user_data/ is gitignored). Counts are
exported post-decay and loaded by the brain as-is; online commits then
accumulate on top (src/brain/src/personal.rs).

Run (from experiments/001-personalization-gain/):

    .\.venv\Scripts\python.exe export_personal.py
    # options: --personal-dir data/personal  --out ../../user_data/personal.tsv
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from pypinyin import Style, lazy_pinyin

from pipeline.corpus import load_personal_documents
from pipeline.personal import build_personal_layer


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def fmt_count(c: float) -> str:
    return str(int(c)) if c == int(c) else f"{c:.6g}"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--personal-dir", default=str(ROOT / "data" / "personal"))
    ap.add_argument(
        "--out",
        default=str(ROOT.parent.parent / "user_data" / "personal.tsv"),
        help="output file (default: <repo>/user_data/personal.tsv)",
    )
    args = ap.parse_args()

    docs = load_personal_documents(args.personal_dir)
    if not docs:
        log(f"WARNING: no documents found in {args.personal_dir}; nothing exported")
        return
    log(f"{len(docs)} personal documents loaded (chronological)")

    # Cold start uses the FULL corpus — unlike the experiment there is no
    # held-out test set to protect.
    layer = build_personal_layer(docs)
    log(
        f"personal layer: {layer.n_train_clauses} clauses, "
        f"{len(layer.lexicon)} lexicon words, {len(layer.zh_oov_words)} zh OOV, "
        f"LM {layer.lm.stats()}"
    )

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    n = 0
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# personal-artifacts-v0 (docs/specs/lm-artifacts-v0.md)\n")
        f.write(f"# built_from={Path(args.personal_dir).name} docs={len(docs)}\n")
        for w, c in sorted(layer.lexicon.items()):
            f.write(f"lex\t{w}\t{fmt_count(c)}\n")
            n += 1
        for low, surface in sorted(layer.case_map.items()):
            if surface != low:  # identity casing carries no information
                f.write(f"case\t{low}\t{surface}\n")
                n += 1
        for w in sorted(layer.zh_oov_words):
            # spaced syllables: the brain derives both the matcher key and
            # the preedit from this (word_to_pinyin_key concatenates, so
            # re-derive with spaces here)
            spaced = " ".join(lazy_pinyin(w, style=Style.NORMAL))
            f.write(f"zhword\t{w}\t{spaced}\n")
            n += 1
        for w, c in sorted(layer.lm.uni.items()):
            if c > 0:
                f.write(f"1\t{w}\t{fmt_count(c)}\n")
                n += 1
        for (w1, w2), c in sorted(layer.lm.bi.items()):
            if c > 0:
                f.write(f"2\t{w1} {w2}\t{fmt_count(c)}\n")
                n += 1
        for (w1, w2, w3), c in sorted(layer.lm.tri.items()):
            if c > 0:
                f.write(f"3\t{w1} {w2} {w3}\t{fmt_count(c)}\n")
                n += 1
    log(f"personal.tsv: {n:,} lines -> {out_path}")


if __name__ == "__main__":
    main()
