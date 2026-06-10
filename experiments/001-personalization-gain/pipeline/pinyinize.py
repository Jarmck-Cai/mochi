"""Hanzi text -> pinyin key stream conversion (mixed Chinese/English aware).

The key stream simulates what the user actually types on a QWERTY keyboard
with no explicit mode switch: hanzi become toneless pinyin letters, English
words stay as-is (lowercased), everything is concatenated without separators.

pypinyin is used with its phrase dictionary so heteronyms are resolved in
context most of the time (e.g. 重庆 -> chong qing, 重要 -> zhong yao).
Note pypinyin already outputs the v-form (lv / nv / lve) which matches the
rime dictionary convention.
"""

from __future__ import annotations

import re
from functools import lru_cache

from pypinyin import lazy_pinyin, Style

from .corpus import EvalItem, HANZI_RE

_SEG_RE = re.compile(r"([一-鿿]+)|([A-Za-z]+)")
_EN_TOKEN_RE = re.compile(r"[A-Za-z]+")


def text_to_syllables(text: str) -> list[str]:
    """Convert mixed text to a list of key units.

    Each unit is either one pinyin syllable (for one hanzi) or one English
    word (lowercased).  Characters outside hanzi/letters are ignored.
    """
    units: list[str] = []
    for m in _SEG_RE.finditer(text):
        hanzi, ascii_word = m.group(1), m.group(2)
        if hanzi:
            units.extend(lazy_pinyin(hanzi, style=Style.NORMAL))
        else:
            units.append(ascii_word.lower())
    return units


def text_to_keystream(text: str) -> str:
    """Full key stream: all units concatenated, no separators."""
    return "".join(text_to_syllables(text))


@lru_cache(maxsize=200_000)
def word_to_pinyin_key(word: str) -> str:
    """Pinyin key string for a dictionary word (used for lexicon arcs)."""
    return "".join(lazy_pinyin(word, style=Style.NORMAL))


def make_eval_item(clause: str, source: str = "") -> EvalItem:
    """Build an EvalItem from a gold clause (with possible inner spaces)."""
    keys = text_to_keystream(clause)
    target = clause.replace(" ", "")
    en_tokens = [t.lower() for t in _EN_TOKEN_RE.findall(clause)]
    is_mixed = bool(en_tokens) and bool(HANZI_RE.search(clause))
    return EvalItem(
        target=target, keys=keys, is_mixed=is_mixed, en_tokens=en_tokens, source=source
    )
