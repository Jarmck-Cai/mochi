"""End-to-end offline experiment: baseline vs baseline + personal layer.

Usage (from experiments/001-personalization-gain/):

    .venv/Scripts/python run_experiment.py             # full run
    .venv/Scripts/python run_experiment.py --quick     # small sizes, smoke test
    .venv/Scripts/python run_experiment.py --personal-dir data/personal

Outputs: console tables + results/metrics.json + results/examples.md
"""

from __future__ import annotations

import argparse
import json
import random
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

# Windows consoles may default to a legacy codepage; we print Chinese tables.
if sys.stdout.encoding and sys.stdout.encoding.lower() not in ("utf-8", "utf8"):
    sys.stdout.reconfigure(encoding="utf-8")

from pipeline.corpus import (
    extract_clauses,
    iter_general_sentences,
    load_personal_documents,
    split_documents,
)
from pipeline.decoder import Scorer, ScorerConfig, decode
from pipeline.evaluate import Metrics, evaluate, format_table
from pipeline.lattice import (
    LatticeBuilder,
    StringMatcher,
    build_english_matcher,
    build_pinyin_matcher,
    load_rime_dict,
)
from pipeline.lm import BackoffTrigramLM
from pipeline.personal import build_personal_layer
from pipeline.pinyinize import make_eval_item


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def train_general_lm(data_dir: Path, max_sentences: int | None, holdout: int):
    """Train the baseline LM on PKU+MSR; hold out tail PKU sentences as a
    general-domain control test set (never seen by any model)."""
    pku = list(iter_general_sentences(data_dir / "general" / "pku_training.utf8"))
    msr = list(
        iter_general_sentences(
            data_dir / "general" / "msr_training.utf8", max_sentences=max_sentences
        )
    )
    control = pku[-holdout:] if holdout else []
    train = pku[: len(pku) - holdout] + msr
    if max_sentences:
        train = train[:max_sentences]
    lm = BackoffTrigramLM()
    for sent in train:
        lm.add_sentence(sent)
    lm.finalize(min_bigram=0.0, min_trigram=1.0)  # drop singleton trigrams
    log(f"general LM trained on {len(train):,} sentences: {lm.stats()}")
    return lm, control


def build_configs() -> dict[str, ScorerConfig]:
    return {
        "baseline(generalLM)": ScorerConfig(mu_general=1.0),
        "+personalLexicon": ScorerConfig(mu_general=1.0, lambda_lexicon=0.8),
        "+personalLM": ScorerConfig(mu_general=0.65, mu_personal=0.35),
        "full(+LM+lexicon)": ScorerConfig(
            mu_general=0.65, mu_personal=0.35, lambda_lexicon=0.8
        ),
    }


def run(args: argparse.Namespace) -> None:
    data_dir = ROOT / "data"
    results_dir = ROOT / "results"
    results_dir.mkdir(exist_ok=True)
    rng = random.Random(args.seed)

    # ---------------------------------------------------------------- corpora
    personal_dir = Path(args.personal_dir) if args.personal_dir else None
    if personal_dir is None:
        real = data_dir / "personal"
        standin = data_dir / "personal_standin"
        has_real = any(
            p.suffix.lower() in (".txt", ".md") and p.name.lower() != "readme.md"
            for p in real.glob("*")
        )
        personal_dir = real if has_real else standin
    log(f"personal corpus: {personal_dir}")
    docs = load_personal_documents(personal_dir)
    if len(docs) < 2:
        sys.exit("need at least 2 personal documents (.txt/.md) to split train/test")
    train_docs, test_docs = split_documents(docs, args.train_ratio)
    log(f"personal docs: {len(train_docs)} train / {len(test_docs)} test")

    # --------------------------------------------------------------- baseline
    max_general = 20_000 if args.quick else args.max_general_sentences
    general_lm, control_sents = train_general_lm(
        data_dir, max_general, holdout=args.control_holdout
    )

    common_english = set()
    en_path = data_dir / "english" / "google-10000-english.txt"
    en_words = [w.strip() for w in en_path.read_text(encoding="utf-8").splitlines()]
    common_english = set(en_words[:2000])
    en_general_matcher = build_english_matcher(set(en_words))

    # --------------------------------------------------------- personal layer
    layer = build_personal_layer(
        train_docs, general_lm=general_lm, common_english=common_english
    )
    log(
        f"personal layer: {layer.n_train_clauses} train clauses, "
        f"lexicon={len(layer.lexicon)}, zh-oov-words={len(layer.zh_oov_words)}, "
        f"terms={len(layer.terms)}; personal LM {layer.lm.stats()}"
    )
    en_personal_matcher = build_english_matcher(
        {w: layer.case_map.get(w, w) for w in layer.lexicon if w.isascii()}
    )

    # ---------------------------------------------------------------- lattice
    entries = load_rime_dict(data_dir / "dict" / "pinyin_simp.dict.yaml")
    char_m, word_m = build_pinyin_matcher(entries, extra_words=layer.zh_oov_words)
    log(
        f"rime dict: {len(entries):,} entries -> "
        f"{len(char_m.entries):,} char keys, {len(word_m.entries):,} word keys"
    )
    builder = LatticeBuilder(
        char_m,
        word_m,
        en_general=en_general_matcher,
        en_personal=en_personal_matcher,
        personal_zh_words=set(layer.zh_oov_words),
    )
    baseline_builder = LatticeBuilder(
        char_m,
        word_m,
        en_general=en_general_matcher,
        en_personal=StringMatcher(),       # baseline has no personal arcs
        personal_zh_words=set(),
    )

    # --------------------------------------------------------------- test set
    test_items = []
    for name, text in test_docs:
        for clause in extract_clauses(text):
            test_items.append(make_eval_item(clause, source=name))
    rng.shuffle(test_items)
    cap = 120 if args.quick else args.max_test_items
    test_items = test_items[:cap]
    log(
        f"personal test set: {len(test_items)} clauses "
        f"({sum(i.is_mixed for i in test_items)} mixed CN/EN)"
    )

    control_items = []
    for sent in control_sents:
        for clause in extract_clauses("".join(sent)):
            control_items.append(make_eval_item(clause, source="pku_control"))
    rng.shuffle(control_items)
    control_items = control_items[: (60 if args.quick else args.max_control_items)]
    log(f"general control set: {len(control_items)} clauses")

    # ----------------------------------------------------------------- decode
    configs = build_configs()
    personal_results: dict[str, Metrics] = {}
    control_results: dict[str, Metrics] = {}
    outputs_by_config: dict[str, list[str]] = {}
    for name, cfg in configs.items():
        scorer = Scorer(general_lm, layer.lm, layer.lexicon, cfg)
        b = baseline_builder if name.startswith("baseline") else builder
        t0 = time.time()
        outs = [decode(it.keys, b, scorer, beam_width=args.beam) for it in test_items]
        dt = time.time() - t0
        outputs_by_config[name] = outs
        personal_results[name] = evaluate(test_items, outs, layer.terms)
        log(
            f"{name}: decoded {len(test_items)} items in {dt:.1f}s "
            f"({1000*dt/max(1,len(test_items)):.0f} ms/item)"
        )
        couts = [decode(it.keys, b, scorer, beam_width=args.beam) for it in control_items]
        control_results[name] = evaluate(control_items, couts, layer.terms)

    # ----------------------------------------------------------------- report
    print("\n## 个人领域测试集（替身语料）\n")
    print(format_table(personal_results))
    print("\n## 通用领域对照集（人民日报 held-out，检查个人层是否伤害通用输入）\n")
    print(format_table(control_results))

    base_out = outputs_by_config["baseline(generalLM)"]
    full_out = outputs_by_config["full(+LM+lexicon)"]
    wins, losses = [], []
    for it, b_o, f_o in zip(test_items, base_out, full_out):
        if f_o == it.target and b_o != it.target:
            wins.append((it, b_o, f_o))
        elif b_o == it.target and f_o != it.target:
            losses.append((it, b_o, f_o))
    print(f"\nfull vs baseline: {len(wins)} wins / {len(losses)} losses")

    examples = ["# 抽样对比（full vs baseline）\n", "## full 赢的例子\n"]
    for it, b_o, f_o in wins[:30]:
        examples.append(f"- 键流 `{it.keys}`\n  - 目标: {it.target}\n  - baseline: {b_o}\n  - full: {f_o}")
    examples.append("\n## full 输的例子\n")
    for it, b_o, f_o in losses[:30]:
        examples.append(f"- 键流 `{it.keys}`\n  - 目标: {it.target}\n  - baseline: {b_o}\n  - full: {f_o}")
    (results_dir / "examples.md").write_text("\n".join(examples), encoding="utf-8")

    payload = {
        "personal_dir": str(personal_dir),
        "quick": args.quick,
        "beam": args.beam,
        "configs": {k: vars(v) | {"arc_prior": None} for k, v in configs.items()},
        "personal_test": {k: m.row() for k, m in personal_results.items()},
        "general_control": {k: m.row() for k, m in control_results.items()},
        "wins": len(wins),
        "losses": len(losses),
    }
    (results_dir / "metrics.json").write_text(
        json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    log("results written to results/metrics.json and results/examples.md")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--quick", action="store_true", help="small sizes, smoke test")
    ap.add_argument("--personal-dir", default=None, help="override personal corpus dir")
    ap.add_argument("--train-ratio", type=float, default=0.8)
    ap.add_argument("--beam", type=int, default=12)
    ap.add_argument("--seed", type=int, default=29)
    ap.add_argument("--max-general-sentences", type=int, default=None,
                    help="cap LM training sentences (default: use all)")
    ap.add_argument("--max-test-items", type=int, default=800)
    ap.add_argument("--max-control-items", type=int, default=200)
    ap.add_argument("--control-holdout", type=int, default=400,
                    help="PKU tail sentences reserved as general control set")
    run(ap.parse_args())


if __name__ == "__main__":
    main()
