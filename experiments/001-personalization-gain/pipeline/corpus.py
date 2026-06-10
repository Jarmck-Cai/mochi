"""Corpus loading, cleaning and train/test splitting.

Two kinds of corpora are handled here:

1. Personal corpus: plain .txt / .md documents (the real user corpus later,
   a stand-in technical corpus for now).  Documents are ordered by file name,
   which is treated as chronological order; the split is done on document
   boundaries so the test set is strictly "in the future" of the train set.

2. General corpus: pre-segmented text (one sentence/paragraph per line,
   words separated by whitespace, e.g. SIGHAN bakeoff training files) used to
   train the baseline language model.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

HANZI_RE = re.compile(r"[一-鿿]")
# A clause is kept only if, after cleaning, it consists of hanzi, ASCII
# letters and inner spaces (spaces only between English words).
VALID_CLAUSE_RE = re.compile(r"^[一-鿿A-Za-z ]+$")
# Punctuation and any other symbol acts as a clause boundary.
CLAUSE_SPLIT_RE = re.compile(r"[^一-鿿A-Za-z ]+")

MD_CODE_BLOCK_RE = re.compile(r"```.*?```", re.S)
MD_MATH_BLOCK_RE = re.compile(r"\$\$.*?\$\$", re.S)
MD_INLINE_MATH_RE = re.compile(r"\$[^$]*\$")
MD_IMAGE_RE = re.compile(r"!\[[^\]]*\]\([^)]*\)")
MD_LINK_RE = re.compile(r"\[([^\]]*)\]\([^)]*\)")
MD_INLINE_CODE_RE = re.compile(r"`([^`]*)`")
MD_HTML_TAG_RE = re.compile(r"<[^>]+>")


@dataclass
class EvalItem:
    """One evaluation unit: a clause the user would type in one go."""

    target: str          # gold text, spaces removed (IME output has no spaces)
    keys: str            # pinyin/letter key stream actually typed
    is_mixed: bool       # contains both hanzi and English letters
    en_tokens: list[str] = field(default_factory=list)  # gold English tokens
    source: str = ""     # document the clause came from


def clean_markdown(text: str) -> str:
    """Strip markdown/code/math noise, keep natural-language text."""
    text = MD_CODE_BLOCK_RE.sub(" ", text)
    text = MD_MATH_BLOCK_RE.sub(" ", text)
    text = MD_IMAGE_RE.sub(" ", text)
    text = MD_LINK_RE.sub(r"\1", text)
    # Keep inline-code content only when it looks like a plain word the user
    # would actually type (e.g. `dropout`), drop code-ish fragments.
    text = MD_INLINE_CODE_RE.sub(
        lambda m: m.group(1) if re.fullmatch(r"[A-Za-z][A-Za-z ]{0,30}", m.group(1)) else " ",
        text,
    )
    text = MD_INLINE_MATH_RE.sub(" ", text)
    text = MD_HTML_TAG_RE.sub(" ", text)
    lines = []
    for line in text.splitlines():
        s = line.strip()
        # Drop markdown directives, headers, tables, d2l ":label:" lines etc.
        if not s or s.startswith(("#", ":", "|", ">", "---")):
            continue
        lines.append(s)
    return "\n".join(lines)


def extract_clauses(text: str, min_hanzi: int = 2, max_chars: int = 22) -> list[str]:
    """Split cleaned text into typeable clauses (punctuation = boundary)."""
    clauses = []
    for raw in CLAUSE_SPLIT_RE.split(text):
        s = re.sub(r"\s+", " ", raw).strip()
        if not s or not VALID_CLAUSE_RE.match(s):
            continue
        n_hanzi = len(HANZI_RE.findall(s))
        if n_hanzi < min_hanzi:
            continue  # pure/mostly-English fragments are not IME conversions
        if len(s.replace(" ", "")) > max_chars or len(s.replace(" ", "")) < 2:
            continue
        # Spaces are only legal between two English letters.
        if re.search(r"(?<![A-Za-z]) | (?![A-Za-z])", s):
            continue
        clauses.append(s)
    return clauses


def load_personal_documents(directory: str | Path) -> list[tuple[str, str]]:
    """Load .txt/.md documents sorted by file name (= chronological order).

    Returns list of (doc_name, cleaned_text).
    """
    directory = Path(directory)
    docs = []
    for path in sorted(directory.glob("*")):
        if path.suffix.lower() not in (".txt", ".md") or path.name.lower() == "readme.md":
            continue
        raw = path.read_text(encoding="utf-8", errors="ignore")
        cleaned = clean_markdown(raw) if path.suffix.lower() == ".md" else raw
        docs.append((path.name, cleaned))
    return docs


def split_documents(
    docs: list[tuple[str, str]], train_ratio: float = 0.8
) -> tuple[list[tuple[str, str]], list[tuple[str, str]]]:
    """Document-level chronological split (no leakage from test to train)."""
    k = max(1, min(len(docs) - 1, round(len(docs) * train_ratio)))
    return docs[:k], docs[k:]


def iter_general_sentences(
    path: str | Path, max_sentences: int | None = None
) -> Iterable[list[str]]:
    """Yield token lists from a pre-segmented corpus file.

    Punctuation tokens split a line into multiple sentences; tokens that are
    not pure hanzi/ASCII words are dropped.
    """
    n = 0
    with open(path, encoding="utf-8", errors="ignore") as f:
        for line in f:
            sent: list[str] = []
            for tok in line.split():
                # PKU-98 style corpora may carry POS tags ("word/pos").
                tok = tok.split("/", 1)[0]
                if VALID_CLAUSE_RE.match(tok) and " " not in tok:
                    sent.append(tok)
                else:
                    if len(sent) >= 2:
                        yield sent
                        n += 1
                        if max_sentences and n >= max_sentences:
                            return
                    sent = []
            if len(sent) >= 2:
                yield sent
                n += 1
                if max_sentences and n >= max_sentences:
                    return
