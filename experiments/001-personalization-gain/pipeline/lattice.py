"""Lattice construction: pinyin word arcs + English word arcs over a key stream.

The lattice is the core data structure of the future translator (DESIGN.md
section 3): for the current key stream we lay three kinds of arcs and let a
single scoring function arbitrate between them in the decoder:

  - pinyin arcs: dictionary words whose concatenated toneless pinyin matches
    a substring of the key stream (includes single-character arcs, so any
    all-pinyin stream is always decodable);
  - English arcs: words from a general English vocabulary and from the
    personal lexicon (this is where `gan`-the-pinyin competes with
    `GAN`-the-term);
  - fallback arcs: single raw letters with a large penalty, guaranteeing the
    lattice is connected even for out-of-vocabulary letter sequences.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

ARC_PY_WORD = "py_word"      # multi-char dictionary word via pinyin
ARC_PY_CHAR = "py_char"      # single hanzi via pinyin
ARC_EN_GENERAL = "en_word"   # general-vocabulary English word
ARC_EN_PERSONAL = "en_personal"  # personal-lexicon English word
ARC_PY_PERSONAL = "py_personal"  # personal-lexicon Chinese word (OOV in dict)
ARC_FALLBACK = "fallback"    # single raw letter, last resort


@dataclass
class Arc:
    start: int
    end: int
    word: str       # surface form emitted when the arc is taken
    kind: str
    lm_token: str = ""  # token used for LM lookup (lowercase for English)

    def __post_init__(self) -> None:
        if not self.lm_token:
            self.lm_token = self.word.lower() if self.word.isascii() else self.word


class StringMatcher:
    """Exact-match dictionary over strings with prefix-based early stopping."""

    def __init__(self) -> None:
        self.entries: dict[str, list[str]] = {}
        self.prefixes: set[str] = set()
        self.max_len = 0

    def add(self, key: str, word: str) -> None:
        if not key:
            return
        bucket = self.entries.setdefault(key, [])
        if word not in bucket:
            bucket.append(word)
        for i in range(1, len(key) + 1):
            self.prefixes.add(key[:i])
        self.max_len = max(self.max_len, len(key))

    def matches_from(self, s: str, start: int):
        """Yield (end, words) for every dictionary key matching s[start:end]."""
        limit = min(len(s), start + self.max_len)
        for end in range(start + 1, limit + 1):
            sub = s[start:end]
            if sub not in self.prefixes:
                break
            words = self.entries.get(sub)
            if words:
                yield end, words


def load_rime_dict(path: str | Path, max_word_len: int = 6) -> list[tuple[str, str, int]]:
    """Parse a rime *.dict.yaml file -> [(word, pinyin_key, weight)].

    Lines look like: 词语<TAB>ci yu<TAB>123 ; the YAML header ends with '...'.
    """
    entries = []
    in_body = False
    hanzi_re = re.compile(r"^[一-鿿]+$")
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not in_body:
                if line.strip() == "...":
                    in_body = True
                continue
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 2:
                continue
            word, pinyin = parts[0], parts[1]
            if not hanzi_re.match(word) or len(word) > max_word_len:
                continue
            weight = 0
            if len(parts) >= 3:
                try:
                    weight = int(float(parts[2]))
                except ValueError:
                    weight = 0
            entries.append((word, pinyin.replace(" ", ""), weight))
    return entries


def build_pinyin_matcher(
    entries: list[tuple[str, str, int]],
    extra_words: dict[str, str] | None = None,
    max_words_per_key: int = 8,
) -> tuple[StringMatcher, StringMatcher]:
    """Build (char_matcher, word_matcher) from rime entries.

    For each pinyin key only the top-N words by rime weight are kept — the
    LM does the real ranking, the dictionary just proposes candidates.
    ``extra_words`` maps personal Chinese words -> pinyin key.
    """
    char_m, word_m = StringMatcher(), StringMatcher()
    by_key: dict[str, list[tuple[int, str]]] = {}
    char_by_key: dict[str, list[tuple[int, str]]] = {}
    for word, key, weight in entries:
        target = char_by_key if len(word) == 1 else by_key
        target.setdefault(key, []).append((weight, word))
    for key, cands in char_by_key.items():
        cands.sort(reverse=True)
        for _, w in cands[: max_words_per_key * 2]:  # chars need more variety
            char_m.add(key, w)
    for key, cands in by_key.items():
        cands.sort(reverse=True)
        for _, w in cands[:max_words_per_key]:
            word_m.add(key, w)
    if extra_words:
        for word, key in extra_words.items():
            word_m.add(key, word)
    return char_m, word_m


def build_english_matcher(words: set[str] | dict[str, str]) -> StringMatcher:
    """Match keys are lowercase; the emitted surface keeps preferred casing
    when ``words`` is a dict mapping lowercase -> preferred form."""
    m = StringMatcher()
    surface_of = words if isinstance(words, dict) else {w: w for w in words}
    for w, surface in surface_of.items():
        w = w.lower()
        if len(w) >= 2 and w.isalpha():
            m.add(w, surface)
    return m


class LatticeBuilder:
    def __init__(
        self,
        char_matcher: StringMatcher,
        word_matcher: StringMatcher,
        en_general: StringMatcher,
        en_personal: StringMatcher | None = None,
        personal_zh_words: set[str] | None = None,
    ):
        self.char_matcher = char_matcher
        self.word_matcher = word_matcher
        self.en_general = en_general
        self.en_personal = en_personal or StringMatcher()
        self.personal_zh_words = personal_zh_words or set()

    def build(self, keys: str) -> list[list[Arc]]:
        """Return arcs grouped by start position; every position is covered."""
        n = len(keys)
        arcs: list[list[Arc]] = [[] for _ in range(n)]
        for i in range(n):
            out = arcs[i]
            for end, words in self.char_matcher.matches_from(keys, i):
                for w in words:
                    out.append(Arc(i, end, w, ARC_PY_CHAR))
            for end, words in self.word_matcher.matches_from(keys, i):
                for w in words:
                    kind = (
                        ARC_PY_PERSONAL if w in self.personal_zh_words else ARC_PY_WORD
                    )
                    out.append(Arc(i, end, w, kind))
            seen_en: set[tuple[int, str]] = set()
            for end, words in self.en_personal.matches_from(keys, i):
                for w in words:
                    out.append(Arc(i, end, w, ARC_EN_PERSONAL))
                    seen_en.add((end, w))
            for end, words in self.en_general.matches_from(keys, i):
                for w in words:
                    if (end, w) not in seen_en:
                        out.append(Arc(i, end, w, ARC_EN_GENERAL))
            if not out:
                out.append(Arc(i, i + 1, keys[i], ARC_FALLBACK))
        return arcs
