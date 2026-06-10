"""Build the personal layer from the user's (or stand-in) corpus.

Outputs of this module:
  - a personal trigram LM with time-decayed counts (newer documents weigh
    more, README of experiment 001: "个人 n-gram（train 集统计，时间衰减）");
  - a personal lexicon word -> decayed count (English terms + Chinese words),
    used both for lexicon-bonus scoring and for adding lattice arcs;
  - a domain term list (train-set only, no test leakage) used by evaluation.
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass, field

import jieba

from .corpus import extract_clauses
from .lm import BackoffTrigramLM
from .pinyinize import word_to_pinyin_key

_HANZI_WORD_RE = re.compile(r"^[一-鿿]{1,6}$")
_EN_WORD_RE = re.compile(r"^[a-zA-Z]{2,20}$")


def tokenize(clause: str) -> list[str]:
    """jieba word segmentation; keeps English runs as single tokens."""
    toks = []
    for t in jieba.cut(clause):
        t = t.strip()
        if not t:
            continue
        if _HANZI_WORD_RE.match(t):
            toks.append(t)
        elif _EN_WORD_RE.match(t):
            toks.append(t.lower())
    return toks


@dataclass
class PersonalLayer:
    lm: BackoffTrigramLM
    lexicon: dict[str, float]                 # word -> decayed count
    zh_oov_words: dict[str, str] = field(default_factory=dict)  # word -> pinyin key
    case_map: dict[str, str] = field(default_factory=dict)  # "resnet" -> "ResNet"
    terms: set[str] = field(default_factory=set)
    n_train_clauses: int = 0


def build_personal_layer(
    train_docs: list[tuple[str, str]],
    general_lm: BackoffTrigramLM | None = None,
    common_english: set[str] | None = None,
    doc_decay: float = 0.95,
    min_en_count: float = 2.0,
    min_zh_count: float = 3.0,
    max_terms: int = 200,
) -> PersonalLayer:
    """train_docs: chronologically ordered (name, text); last doc = newest."""
    common_english = common_english or set()
    lm = BackoffTrigramLM()
    word_counts: Counter = Counter()
    surface_counts: Counter = Counter()  # (lower, original-case form) -> count
    n_docs = len(train_docs)
    n_clauses = 0
    for idx, (_name, text) in enumerate(train_docs):
        weight = doc_decay ** (n_docs - 1 - idx)  # newest doc -> weight 1.0
        for clause in extract_clauses(text):
            toks = tokenize(clause)
            if not toks:
                continue
            lm.add_sentence(toks, weight)
            for t in toks:
                word_counts[t] += weight
            for m in re.finditer(r"[A-Za-z]{2,20}", clause):
                surface_counts[(m.group(0).lower(), m.group(0))] += weight
            n_clauses += 1
    lm.finalize()

    # Preferred written form of each English term (e.g. resnet -> ResNet):
    # the IME should learn the user's casing habits, not just the letters.
    case_best: dict[str, tuple[float, str]] = {}
    for (low, surface), c in surface_counts.items():
        if c > case_best.get(low, (0.0, ""))[0]:
            case_best[low] = (c, surface)
    case_map = {low: surface for low, (_c, surface) in case_best.items()}

    # Personal lexicon: recurring words, English terms and Chinese words alike.
    lexicon: dict[str, float] = {}
    for w, c in word_counts.items():
        if w.isascii():
            if c >= min_en_count:
                lexicon[w] = c
        elif len(w) >= 2 and c >= min_zh_count:
            lexicon[w] = c

    # Chinese lexicon words need pinyin keys so the lattice can propose them
    # even when the static dictionary does not contain them.
    zh_oov_words = {
        w: word_to_pinyin_key(w) for w in lexicon if not w.isascii()
    }

    # Domain term list for evaluation: strongly over-represented vs general.
    terms: list[tuple[float, str]] = []
    for w, c in word_counts.items():
        if w.isascii():
            if len(w) >= 3 and c >= 5 and w not in common_english:
                terms.append((c, w))
        elif len(w) >= 2 and c >= 8:
            if general_lm is None:
                terms.append((c, w))
            else:
                g = general_lm.uni.get(w, 0.0)
                p_personal = c / max(1.0, sum(word_counts.values()))
                p_general = (g + 0.5) / max(1.0, general_lm.total)
                if p_personal / p_general >= 20.0:
                    terms.append((c, w))
    terms.sort(reverse=True)
    term_set = {w for _, w in terms[:max_terms]}

    return PersonalLayer(
        lm=lm,
        lexicon=lexicon,
        zh_oov_words=zh_oov_words,
        case_map=case_map,
        terms=term_set,
        n_train_clauses=n_clauses,
    )
