"""Evaluation metrics for the offline IME experiment.

Metrics (README of experiment 001):
  - sentence_top1: exact match of the decoded first candidate vs gold clause;
  - char_acc: corpus-level 1 - edit_distance / gold_length;
  - term_hit: among occurrences of personal-domain terms in gold clauses,
    fraction reproduced verbatim in the decoded output;
  - mixed_top1 / en_token_recall: sentence accuracy restricted to clauses
    mixing hanzi and English, and recall of gold English tokens there.
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass

from .corpus import EvalItem


def levenshtein(a: str, b: str) -> int:
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[-1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


@dataclass
class Metrics:
    n_items: int
    sentence_top1: float
    char_acc: float
    term_hit: float
    term_occurrences: int
    mixed_top1: float
    en_token_recall: float
    n_mixed: int

    def row(self) -> dict:
        return {
            "items": self.n_items,
            "sentence_top1": round(self.sentence_top1, 4),
            "char_acc": round(self.char_acc, 4),
            "term_hit": round(self.term_hit, 4),
            "term_occ": self.term_occurrences,
            "mixed_top1": round(self.mixed_top1, 4),
            "en_token_recall": round(self.en_token_recall, 4),
            "mixed_items": self.n_mixed,
        }


def evaluate(
    items: list[EvalItem], outputs: list[str], terms: set[str]
) -> Metrics:
    assert len(items) == len(outputs)
    n = len(items)
    hits = 0
    edit_total = 0
    len_total = 0
    term_occ = 0
    term_hits = 0
    mixed_n = 0
    mixed_hits = 0
    en_total = 0
    en_recalled = 0
    zh_terms = [t for t in terms if not t.isascii()]
    en_terms = {t.lower() for t in terms if t.isascii()}
    for item, out in zip(items, outputs):
        gold = item.target
        if out == gold:
            hits += 1
        edit_total += levenshtein(out, gold)
        len_total += len(gold)
        out_lower = out.lower()
        # Chinese terms: substring occurrences in gold vs reproduced in output.
        for t in zh_terms:
            c = gold.count(t)
            if c:
                term_occ += c
                term_hits += min(c, out.count(t))
        # English terms: token-level (gold tokens recorded at item build time).
        gold_en = Counter(item.en_tokens)
        out_en = Counter(m.group(0) for m in re.finditer(r"[a-z]+", out_lower))
        for tok, c in gold_en.items():
            if tok in en_terms:
                term_occ += c
                term_hits += min(c, out_en.get(tok, 0))
        if item.is_mixed:
            mixed_n += 1
            if out == gold:
                mixed_hits += 1
            for tok, c in gold_en.items():
                en_total += c
                en_recalled += min(c, out_en.get(tok, 0))
    return Metrics(
        n_items=n,
        sentence_top1=hits / n if n else 0.0,
        char_acc=1.0 - edit_total / len_total if len_total else 0.0,
        term_hit=term_hits / term_occ if term_occ else 0.0,
        term_occurrences=term_occ,
        mixed_top1=mixed_hits / mixed_n if mixed_n else 0.0,
        en_token_recall=en_recalled / en_total if en_total else 0.0,
        n_mixed=mixed_n,
    )


def format_table(rows: dict[str, Metrics]) -> str:
    """Render configs x metrics as a markdown table (percentages)."""
    headers = [
        "config", "items", "整句首选命中率", "字级准确率",
        "术语命中率", "中英段整句命中", "英文词召回",
    ]
    lines = ["| " + " | ".join(headers) + " |",
             "|" + "---|" * len(headers)]
    for name, m in rows.items():
        lines.append(
            f"| {name} | {m.n_items} | {m.sentence_top1:.1%} | {m.char_acc:.1%} "
            f"| {m.term_hit:.1%} | {m.mixed_top1:.1%} | {m.en_token_recall:.1%} |"
        )
    return "\n".join(lines)
