"""Decode key streams using the artifacts/v0 TSVs with the pipeline classes.

Parity oracle for the Rust brain decoder port (M2): both sides load the SAME
lm-artifacts-v0 files and run the SAME algorithm (lattice + stupid-backoff
trigram + beam search, general layer only), so top-1 outputs should agree.

    .\.venv\Scripts\python.exe decode_from_artifacts.py nihao woyaoceshi ...

Default artifacts dir: ../../artifacts/v0
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from pipeline.lattice import (
    LatticeBuilder,
    build_english_matcher,
    build_pinyin_matcher,
)
from pipeline.decoder import Scorer, ScorerConfig, decode
from pipeline.lm import BackoffTrigramLM, EmptyLM


def load_lm(path: Path) -> BackoffTrigramLM:
    alpha, total = None, None
    lm = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("#"):
                if "total_tokens=" in line:
                    total = float(line.split("total_tokens=")[1])
                if "backoff=stupid:" in line:
                    alpha = float(line.split("backoff=stupid:")[1])
                continue
            if lm is None:
                assert alpha is not None and total is not None, "missing headers"
                lm = BackoffTrigramLM(alpha=alpha)
                lm.total = total
            order, tokens, count = line.split("\t")
            toks = tokens.split(" ")
            c = float(count)
            if order == "1":
                lm.uni[toks[0]] = lm.uni.get(toks[0], 0.0) + c
            elif order == "2":
                lm.bi[(toks[0], toks[1])] = lm.bi.get((toks[0], toks[1]), 0.0) + c
            else:
                k = (toks[0], toks[1], toks[2])
                lm.tri[k] = lm.tri.get(k, 0.0) + c
    assert lm is not None
    lm.finalize()
    return lm


def load_entries(path: Path) -> list[tuple[str, str, int]]:
    entries = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#"):
                continue
            key, text, weight = line.rstrip("\n").split("\t")
            entries.append((text, key.replace(" ", ""), int(float(weight))))
    return entries


def load_english(path: Path) -> set[str]:
    words = set()
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#"):
                continue
            words.add(line.split("\t")[0].strip())
    return words


def main() -> None:
    keystreams = sys.argv[1:] or [
        "nihao",
        "zhongwenshuru",
        "woyaoceshi",
        "jintiantianqihenhao",
        "woaibeijingtiananmen",
        "zhongguorenminyinhang",
        "xianzaikaishi",
    ]
    art = ROOT.parent.parent / "artifacts" / "v0"
    t0 = time.time()
    lm = load_lm(art / "ngram.tsv")
    char_m, word_m = build_pinyin_matcher(load_entries(art / "dict.tsv"))
    en_m = build_english_matcher(load_english(art / "english.tsv"))
    builder = LatticeBuilder(char_m, word_m, en_m)
    scorer = Scorer(lm, EmptyLM(), {}, ScorerConfig())
    print(f"# loaded artifacts in {time.time() - t0:.1f}s ({lm.stats()})")
    for keys in keystreams:
        t0 = time.time()
        out = decode(keys, builder, scorer, beam_width=12)
        ms = (time.time() - t0) * 1000
        print(f"{keys}\t{out}\t{ms:.1f}ms")


if __name__ == "__main__":
    main()
