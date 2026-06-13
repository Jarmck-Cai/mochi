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
ENGLISH_SOURCE = "wordfreq top-50k (en), zipf frequencies"
ESSAY_SOURCE = "rime essay 八股文 (data/dict/essay.txt, opencc t2s)"


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def fmt_count(c: float) -> str:
    """Counts are floats by contract (weighted counts allowed); emit ints cleanly."""
    return str(int(c)) if c == int(c) else f"{c:g}"


# ------------------------------------------------------------------ essay
def load_essay(path: Path) -> dict[str, float]:
    """rime essay (八股文, traditional) -> simplified word -> weight.

    The IME-grade common-word base the SIGHAN news corpus lacks: modern
    word frequencies + compounds the rime dict has no entry for ("常用词").
    Traditional variants mapping to one simplified form are summed.
    """
    import re

    from opencc import OpenCC

    t2s = OpenCC("t2s")
    hanzi_re = re.compile(r"^[一-鿿]+$")
    words: dict[str, float] = {}
    n_raw = 0
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) != 2:
                continue
            word, weight = parts
            try:
                w = float(weight)
            except ValueError:
                continue
            if w <= 0 or not hanzi_re.match(word):
                continue
            simp = t2s.convert(word)
            if not hanzi_re.match(simp):
                continue
            words[simp] = words.get(simp, 0.0) + w
            n_raw += 1
    log(f"essay: {n_raw:,} raw entries -> {len(words):,} simplified hanzi words")
    return words


# ----------------------------------------------------------------- dict.tsv
def export_dict(
    data_dir: Path, out_path: Path, essay: dict[str, float] | None = None
) -> tuple[int, int]:
    entries = load_rime_dict(
        data_dir / "dict" / "pinyin_simp.dict.yaml", join_syllables=False
    )
    # Merge duplicate (key, text) pairs, keeping the highest rime weight.
    merged: dict[tuple[str, str], int] = {}
    for text, key, weight in entries:
        k = (key, text)
        if weight > merged.get(k, -1):
            merged[k] = weight

    # Essay compounds the rime dict misses ("常用词") get entries of their
    # own — without a whole-word arc the decoder can only assemble them
    # char by char against a news-corpus LM that has never seen them.
    n_essay = 0
    if essay:
        from pypinyin import Style, lazy_pinyin

        known_texts = {text for (_k, text) in merged}
        # scale essay weights into the rime-dict magnitude (weights only
        # gate the per-key top-N preselection; the LM does the ranking)
        s = sum(merged.values()) / max(1.0, sum(essay.values()))
        for word, w in essay.items():
            if not (2 <= len(word) <= 7) or word in known_texts:
                continue
            syls = lazy_pinyin(word, style=Style.NORMAL)
            if len(syls) != len(word) or not all(
                s_.isascii() and s_.isalpha() for s_ in syls
            ):
                continue  # unreadable char or odd pinyin: skip
            key = " ".join(syls)
            merged[(key, word)] = max(1, round(w * s))
            n_essay += 1
        log(f"dict.tsv: +{n_essay:,} essay compounds (scale={s:.4f})")

    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# key<TAB>text<TAB>weight\n")
        for (key, text), weight in sorted(merged.items()):
            f.write(f"{key}\t{text}\t{weight}\n")
    log(f"dict.tsv: {len(merged):,} entries ({len(entries):,} raw rime lines)")
    return len(merged), n_essay


# ---------------------------------------------------------------- ngram.tsv
def train_full_general_lm(
    data_dir: Path, extra: list[tuple[Path, float]] | None = None
) -> BackoffTrigramLM:
    """Same recipe as run_experiment.train_general_lm but full corpus:
    no control holdout, no sentence cap.

    `extra` is a list of (pre-segmented corpus file, per-sentence weight) for
    相位 2 modern-corpus refresh: SIGHAN (2005) is huge, so a small modern
    corpus needs weight > 1 to actually shift rankings. Same token format as
    SIGHAN (space-separated words); changes must pass the gate before shipping.
    """
    lm = BackoffTrigramLM()  # alpha=0.4, matching the experiment
    n = 0
    for name in ("pku_training.utf8", "msr_training.utf8"):
        for sent in iter_general_sentences(data_dir / "general" / name):
            lm.add_sentence(sent)
            n += 1
    for path, weight in extra or []:
        m = 0
        for sent in iter_general_sentences(path):
            lm.add_sentence(sent, weight=weight)
            m += 1
        log(f"extra corpus {Path(path).name}: +{m:,} sentences (weight={weight})")
    lm.finalize(min_bigram=0.0, min_trigram=1.0)  # drop singleton trigrams
    log(f"general LM trained on {n:,} base sentences: {lm.stats()}")
    return lm


def blend_unigrams(
    lm: BackoffTrigramLM,
    counts: dict[str, float],
    lam: float,
    base_total: float,
    label: str,
) -> float:
    """Mix an external word-frequency table into the unigram rung.

    new_uni(w) = corpus(w) + freq(w) · (base_total/Σfreq) · λ
    so λ is this source's share relative to the corpus mass. Returns the
    added mass (base_total·λ); the caller updates lm.total once after all
    blends. Bigram/trigram paths and their denominators stay corpus-pure
    (frequency tables carry no context information).
    """
    factor = base_total * lam / max(1.0, sum(counts.values()))
    for w, c in counts.items():
        lm.uni[w] = lm.uni.get(w, 0.0) + c * factor
    added = base_total * lam
    log(f"unigram blend [{label}]: λ={lam}, {len(counts):,} words, +{added:,.0f} mass")
    return added


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
def export_english(out_path: Path) -> tuple[int, dict[str, float]]:
    """wordfreq top-50k with zipf-derived frequencies.

    Returns (word count, word -> relative frequency) — the frequencies feed
    the unigram blend so English words rank against each other (the old
    10k list had no usable weights; every English word scored the OOV
    floor, so e.g. congratulations vs junk parses was a coin flip).
    """
    import re

    from wordfreq import top_n_list, zipf_frequency

    word_re = re.compile(r"^[a-z]{2,20}$")
    freqs: dict[str, float] = {}
    rank = 0
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("# word<TAB>rank\n")
        for w in top_n_list("en", 50000):
            if not word_re.match(w):
                continue
            rank += 1
            f.write(f"{w}\t{rank}\n")
            freqs[w] = 10.0 ** zipf_frequency(w, "en")  # relative scale
    log(f"english.tsv: {rank:,} words (wordfreq top-50k)")
    return rank, freqs


# ------------------------------------------------------------------- driver
def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--out",
        default=str(ROOT.parent.parent / "artifacts" / "v0"),
        help="output directory (default: <repo>/artifacts/v0)",
    )
    ap.add_argument(
        "--essay",
        default=str(ROOT / "data" / "dict" / "essay.txt"),
        help="rime essay.txt path (common-word base; copy from a Weasel "
        "install's data/ dir); pass an empty string to disable",
    )
    ap.add_argument(
        "--essay-lambda",
        type=float,
        default=1.0,
        help="essay share of unigram mass relative to the corpus (1.0 = 50/50)",
    )
    ap.add_argument(
        "--english-lambda",
        type=float,
        default=0.3,
        help="english-frequency share of unigram mass (typing is mostly Chinese)",
    )
    ap.add_argument(
        "--extra-corpus",
        action="append",
        default=[],
        metavar="PATH",
        help="extra pre-segmented corpus (相位 2 modern refresh); repeatable",
    )
    ap.add_argument(
        "--extra-weight",
        type=float,
        default=1.0,
        help="per-sentence weight for --extra-corpus (SIGHAN is large; use >1 to shift)",
    )
    args = ap.parse_args()

    data_dir = ROOT / "data"
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    t0 = time.time()

    essay = None
    if args.essay and Path(args.essay).is_file():
        essay = load_essay(Path(args.essay))
    elif args.essay:
        log(f"WARNING: essay file not found at {args.essay}; building without it")

    extra = [(Path(p), args.extra_weight) for p in args.extra_corpus]
    for p, _ in extra:
        if not p.is_file():
            sys.exit(f"--extra-corpus not found: {p}")

    n_dict, n_essay_words = export_dict(data_dir, out_dir / "dict.tsv", essay)
    n_english, english_freqs = export_english(out_dir / "english.tsv")
    lm = train_full_general_lm(data_dir, extra)
    base_total = lm.total
    added = 0.0
    if essay:
        added += blend_unigrams(lm, essay, args.essay_lambda, base_total, "essay")
    if english_freqs:
        added += blend_unigrams(
            lm, english_freqs, args.english_lambda, base_total, "english"
        )
    lm.total = base_total + added
    log(f"total_tokens {base_total:,.0f} -> {lm.total:,.0f}")
    ngram_counts = export_ngram(lm, out_dir / "ngram.tsv")

    meta = {
        "v": 0,
        "built_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        "sources": {
            "dict": DICT_SOURCE,
            "corpus": CORPUS_SOURCE,
            "english": ENGLISH_SOURCE,
            **({"essay": ESSAY_SOURCE} if essay else {}),
        },
        "params": {
            "backoff": f"stupid:{lm.alpha}",
            "min_trigram": 1.0,
            **({"essay_lambda": args.essay_lambda} if essay else {}),
            "english_lambda": args.english_lambda,
            **({"extra_corpus": [Path(p).name for p in args.extra_corpus],
                "extra_weight": args.extra_weight} if args.extra_corpus else {}),
        },
        "counts": {
            "dict": n_dict,
            **ngram_counts,
            "english": n_english,
            **({"essay_dict_words": n_essay_words, "essay_words": len(essay)} if essay else {}),
        },
    }
    (out_dir / "meta.json").write_text(
        json.dumps(meta, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    log(f"meta.json written; all artifacts in {out_dir} ({time.time() - t0:.1f}s total)")


if __name__ == "__main__":
    main()
