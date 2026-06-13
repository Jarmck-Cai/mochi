"""Harvest + segment a modern Chinese corpus into SIGHAN-compatible text for
export_artifacts.py --extra-corpus (相位 2, LLM-free).

Source: Leipzig Corpora Collection news slices (zho_news_YYYY, CC BY), one
`<id>\\t<sentence>` per line; download e.g.
    https://downloads.wortschatz-leipzig.de/corpora/zho_news_2020_100K.tar.gz
Segmenter: pkuseg (PKU standard) — chosen over jieba because the base trigram
is trained on SIGHAN PKU+MSR, so PKU-standard segmentation keeps token counts
from splitting across two conventions (jieba≈9.8% vs pkuseg≈3.5% SIGHAN mismatch).

Output: space-separated tokens, one sentence per line — same format
iter_general_sentences() consumes (punctuation tokens act as clause splitters).

Usage:
    python build_modern_corpus.py <leipzig-sentences.txt> [more...] -o modern.txt
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

HANZI = re.compile(r"[一-鿿]")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("inputs", nargs="+", help="Leipzig *-sentences.txt files")
    ap.add_argument("-o", "--out", required=True, help="segmented output path")
    args = ap.parse_args()

    import spacy_pkuseg as pkuseg

    seg = pkuseg.pkuseg()
    n_in = n_out = 0
    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        for path in args.inputs:
            for line in Path(path).read_text(encoding="utf-8", errors="ignore").splitlines():
                # Leipzig is "id<TAB>sentence"; tolerate plain lines too.
                sent = line.split("\t", 1)[-1].strip()
                n_in += 1
                if not HANZI.search(sent):
                    continue
                toks = seg.cut(sent)
                if len(toks) >= 2:
                    f.write(" ".join(toks) + "\n")
                    n_out += 1
    print(f"segmented {n_out}/{n_in} sentences -> {args.out}")


if __name__ == "__main__":
    main()
