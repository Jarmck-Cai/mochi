"""Export general-layer artifacts in lm-artifacts-v0 format (docs/specs/lm-artifacts-v0.md).

Reuses the experiment-001 pipeline code to produce the text exchange files
consumed by `brain build-artifacts`:

    artifacts/v0/dict.tsv      pinyin dictionary  (key<TAB>text<TAB>weight)
    artifacts/v0/ngram.tsv     general trigram counts, stupid backoff alpha=0.4
    artifacts/v0/english.tsv   general English vocabulary (word<TAB>rank)
    artifacts/v0/meta.json     version / sources / entry counts

Unlike run_experiment.train_general_lm, the LM here is trained on the FULL
SIGHAN corpus (PKU + MSR), with no control holdout and no sentence cap.
Singleton trigrams are pruned (same min_trigram=1.0 as the experiment).

Rebuild (from experiments/001-personalization-gain/):

    .\.venv\Scripts\python.exe export_artifacts.py

Optional: --out <dir> to override the output directory (default ../../artifacts/v0).
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from pipeline.corpus import iter_general_sentences
from pipeline.lattice import load_rime_dict
from pipeline.lm import BackoffTrigramLM

DICT_SOURCE = "rime pinyin_simp (data/dict/pinyin_simp.dict.yaml)"
CORPUS_SOURCE = "SIGHAN bakeoff pku+msr (data/general/, full, no holdout)"
ENGLISH_SOURCE = "google-10000-english (data/english/google-10000-english.txt)"


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def fmt_count(c: float) -> str:
    """Counts are floats by contract (weighted counts allowed); emit ints cleanly."""
    return str(int(c)) if c == int(c) else f"{c:g}"


# ----------------------------------------------------------------- dict.tsv
def export_dict(data_dir: Path, out_path: Path) -> int:
    entries = load_rime_dict(
        data_dir / "dict" / "pinyin_simp.dict.yaml", join_syllables=False
    )
    # Merge duplicate (key, text) pairs, keeping the highest rime weight.
    merged: dict[tuple[str, str], int] = {}
    for text, key, weight in entries:
        k = (key, text)
        if weight > merged.get(k, -1):
            merged[k] = weight
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# key<TAB>text<TAB>weight\n")
        for (key, text), weight in sorted(merged.items()):
            f.write(f"{key}\t{text}\t{weight}\n")
    log(f"dict.tsv: {len(merged):,} entries ({len(entries):,} raw rime lines)")
    return len(merged)


# ---------------------------------------------------------------- ngram.tsv
def train_full_general_lm(data_dir: Path) -> BackoffTrigramLM:
    """Same recipe as run_experiment.train_general_lm but full corpus:
    no control holdout, no sentence cap."""
    lm = BackoffTrigramLM()  # alpha=0.4, matching the experiment
    n = 0
    for name in ("pku_training.utf8", "msr_training.utf8"):
        for sent in iter_general_sentences(data_dir / "general" / name):
            lm.add_sentence(sent)
            n += 1
    lm.finalize(min_bigram=0.0, min_trigram=1.0)  # drop singleton trigrams
    log(f"general LM trained on {n:,} sentences: {lm.stats()}")
    return lm


def export_ngram(lm: BackoffTrigramLM, out_path: Path) -> dict[str, int]:
    counts = {"ngram1": 0, "ngram2": 0, "ngram3": 0}
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# order<TAB>token1[ token2[ token3]]<TAB>count\n")
        f.write(f"# total_tokens={fmt_count(lm.total)}\n")
        f.write(f"# backoff=stupid:{lm.alpha}\n")
        for w, c in sorted(lm.uni.items()):
            if c > 0:
                f.write(f"1\t{w}\t{fmt_count(c)}\n")
                counts["ngram1"] += 1
        for (w1, w2), c in sorted(lm.bi.items()):
            if c > 0:
                f.write(f"2\t{w1} {w2}\t{fmt_count(c)}\n")
                counts["ngram2"] += 1
        for (w1, w2, w3), c in sorted(lm.tri.items()):
            if c > 0:
                f.write(f"3\t{w1} {w2} {w3}\t{fmt_count(c)}\n")
                counts["ngram3"] += 1
    log(
        f"ngram.tsv: {counts['ngram1']:,} uni / {counts['ngram2']:,} bi / "
        f"{counts['ngram3']:,} tri (total_tokens={lm.total:,.0f})"
    )
    return counts


# -------------------------------------------------------------- english.tsv
def export_english(data_dir: Path, out_path: Path) -> int:
    words_raw = (
        (data_dir / "english" / "google-10000-english.txt")
        .read_text(encoding="utf-8")
        .splitlines()
    )
    seen: set[str] = set()
    rank = 0
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# word<TAB>rank\n")
        for w in words_raw:
            w = w.strip()
            if not w or w in seen:
                continue
            seen.add(w)
            rank += 1
            f.write(f"{w}\t{rank}\n")
    log(f"english.tsv: {rank:,} words")
    return rank


# ------------------------------------------------------------------- driver
def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--out",
        default=str(ROOT.parent.parent / "artifacts" / "v0"),
        help="output directory (default: <repo>/artifacts/v0)",
    )
    args = ap.parse_args()

    data_dir = ROOT / "data"
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    t0 = time.time()

    n_dict = export_dict(data_dir, out_dir / "dict.tsv")
    lm = train_full_general_lm(data_dir)
    ngram_counts = export_ngram(lm, out_dir / "ngram.tsv")
    n_english = export_english(data_dir, out_dir / "english.tsv")

    meta = {
        "v": 0,
        "built_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "sources": {
            "dict": DICT_SOURCE,
            "corpus": CORPUS_SOURCE,
            "english": ENGLISH_SOURCE,
        },
        "params": {"backoff": f"stupid:{lm.alpha}", "min_trigram": 1.0},
        "counts": {"dict": n_dict, **ngram_counts, "english": n_english},
    }
    (out_dir / "meta.json").write_text(
        json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    log(f"meta.json written; all artifacts in {out_dir} ({time.time() - t0:.1f}s total)")


if __name__ == "__main__":
    main()
