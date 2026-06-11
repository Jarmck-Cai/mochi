"""Personal-layer parity oracle: Python pipeline vs Rust brain (M2-3).

Both sides carry the SAME general artifacts and the SAME personal layer
(Python: build_personal_layer over the full corpus — exactly what
export_personal.py exported to user_data/personal.tsv; Rust: that TSV),
scored with the same full config (mu_g=0.65, mu_p=0.35, lambda_lex=0.8).
Top-1 decodes must agree.

    .\.venv\Scripts\python.exe parity_personal.py [keys ...]

Without arguments, samples clauses from the personal corpus (seeded) and
adds a few generic streams. Only the agree/disagree tally belongs in
reports — clause texts are private.
"""

from __future__ import annotations

import random
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from decode_from_artifacts import load_english, load_entries, load_lm
from pipeline.corpus import extract_clauses, load_personal_documents
from pipeline.decoder import Scorer, ScorerConfig, decode
from pipeline.lattice import LatticeBuilder, build_english_matcher, build_pinyin_matcher
from pipeline.personal import build_personal_layer
from pipeline.pinyinize import make_eval_item

BRAIN = ROOT.parent.parent / "src" / "brain" / "target" / "release" / "mochi-brain.exe"
USER_DATA = ROOT.parent.parent / "user_data"
GENERIC = ["nihao", "woyaoceshi", "jintiantianqihenhao", "zhongwenshuru"]


def sample_corpus_keys(n: int = 8) -> list[str]:
    docs = load_personal_documents(ROOT / "data" / "personal")
    clauses = [c for _, text in docs for c in extract_clauses(text)]
    rng = random.Random(29)
    picked = rng.sample(clauses, min(n * 3, len(clauses)))
    keys = []
    for c in picked:
        item = make_eval_item(c)
        if item.keys and 6 <= len(item.keys) <= 24 and item.keys.isascii():
            keys.append(item.keys)
        if len(keys) >= n:
            break
    return keys


def main() -> None:
    keystreams = sys.argv[1:] or (GENERIC + sample_corpus_keys())
    art = ROOT.parent.parent / "artifacts" / "v0"
    t0 = time.time()
    lm = load_lm(art / "ngram.tsv")
    layer = build_personal_layer(load_personal_documents(ROOT / "data" / "personal"))
    char_m, word_m = build_pinyin_matcher(
        load_entries(art / "dict.tsv"), extra_words=layer.zh_oov_words
    )
    builder = LatticeBuilder(
        char_m,
        word_m,
        en_general=build_english_matcher(load_english(art / "english.tsv")),
        en_personal=build_english_matcher(
            {w: layer.case_map.get(w, w) for w in layer.lexicon if w.isascii()}
        ),
        personal_zh_words=set(layer.zh_oov_words),
    )
    scorer = Scorer(
        lm,
        layer.lm,
        layer.lexicon,
        ScorerConfig(mu_general=0.65, mu_personal=0.35, lambda_lexicon=0.8),
    )
    print(f"# python side ready in {time.time() - t0:.1f}s")

    rust_raw = subprocess.run(
        [str(BRAIN), "--user-data", str(USER_DATA), "--topn", "1", "--decode", *keystreams],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    ).stdout
    rust_top1 = {}
    for line in rust_raw.splitlines():
        cols = line.split("\t")
        if len(cols) >= 3 and cols[1] == "#1":
            rust_top1[cols[0]] = cols[2]

    agree = 0
    for keys in keystreams:
        py = decode(keys, builder, scorer, beam_width=12)
        rs = rust_top1.get(keys, "<missing>")
        ok = py == rs
        agree += ok
        print(f"{'OK ' if ok else 'DIFF'}\t{keys}\tpy={py}\trs={rs}")
    print(f"# top-1 agreement: {agree}/{len(keystreams)}")


if __name__ == "__main__":
    main()
