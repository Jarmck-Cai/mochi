"""Pure-Python word trigram language model with stupid backoff.

KenLM is deliberately avoided (hard to build on Windows); for offline
evaluation at this scale a counting trigram with stupid backoff is enough
and trivially supports weighted (time-decayed) counts for the personal LM.

Probabilities are base-10 log.  ``logp`` implements:

    P(w | h2 h1) = c(h2 h1 w) / c(h2 h1)                 if trigram seen
                 = alpha * c(h1 w) / c(h1)               elif bigram seen
                 = alpha^2 * c(w) / total                elif unigram seen
                 = alpha^2 * floor                       otherwise (OOV)
"""

from __future__ import annotations

import math
from typing import Iterable

BOS = "<s>"

_LOG10 = math.log(10)


class BackoffTrigramLM:
    def __init__(self, alpha: float = 0.4, oov_logp: float = -7.5):
        self.alpha = alpha
        self.log_alpha = math.log10(alpha)
        self.oov_logp = oov_logp  # floor log10-prob for unseen unigrams
        self.uni: dict[str, float] = {}
        self.bi: dict[tuple[str, str], float] = {}
        self.tri: dict[tuple[str, str, str], float] = {}
        self.total = 0.0
        self._logp_cache: dict[tuple[str, str, str], float] = {}

    # ------------------------------------------------------------------ train
    def add_sentence(self, tokens: list[str], weight: float = 1.0) -> None:
        if not tokens:
            return
        uni, bi, tri = self.uni, self.bi, self.tri
        padded = [BOS, BOS] + tokens
        for w in tokens:
            uni[w] = uni.get(w, 0.0) + weight
        self.total += weight * len(tokens)
        for i in range(2, len(padded)):
            b = (padded[i - 1], padded[i])
            bi[b] = bi.get(b, 0.0) + weight
            t = (padded[i - 2], padded[i - 1], padded[i])
            tri[t] = tri.get(t, 0.0) + weight
        # History counts for BOS contexts (needed as trigram denominators).
        h = (padded[0], padded[1])
        bi[h] = bi.get(h, 0.0)  # ensure key exists; denominator handled below

    def fit(self, sentences: Iterable[list[str]], weight_fn=None) -> "BackoffTrigramLM":
        for i, sent in enumerate(sentences):
            w = weight_fn(i) if weight_fn else 1.0
            self.add_sentence(sent, w)
        return self

    def finalize(self, min_bigram: float = 0.0, min_trigram: float = 0.0) -> None:
        """Prune rare higher-order counts to save memory, build denominators."""
        if min_trigram > 0:
            self.tri = {k: v for k, v in self.tri.items() if v > min_trigram}
        if min_bigram > 0:
            self.bi = {k: v for k, v in self.bi.items() if v > min_bigram}
        # Denominator tables (recomputed from the *unpruned semantics*: we use
        # pruned numerators over consistent denominators, which keeps the
        # model a proper scoring function even if not a true distribution —
        # fine for stupid backoff which is unnormalised anyway).
        self.bi_hist: dict[tuple[str, str], float] = {}
        for (w1, w2, _), c in self.tri.items():
            k = (w1, w2)
            self.bi_hist[k] = self.bi_hist.get(k, 0.0) + c
        self.uni_hist: dict[str, float] = {}
        for (w1, _), c in self.bi.items():
            self.uni_hist[w1] = self.uni_hist.get(w1, 0.0) + c

    # ------------------------------------------------------------------ score
    def logp(self, w: str, h2: str, h1: str) -> float:
        key = (h2, h1, w)
        cached = self._logp_cache.get(key)
        if cached is not None:
            return cached
        val = self._logp_uncached(w, h2, h1)
        if len(self._logp_cache) < 2_000_000:
            self._logp_cache[key] = val
        return val

    def _logp_uncached(self, w: str, h2: str, h1: str) -> float:
        c3 = self.tri.get((h2, h1, w))
        if c3:
            denom = self.bi_hist.get((h2, h1), 0.0)
            if denom:
                return math.log10(c3 / denom)
        c2 = self.bi.get((h1, w))
        if c2:
            denom = self.uni_hist.get(h1, 0.0)
            if denom:
                return self.log_alpha + math.log10(c2 / denom)
        c1 = self.uni.get(w)
        if c1 and self.total:
            return 2 * self.log_alpha + math.log10(c1 / self.total)
        return 2 * self.log_alpha + self.oov_logp

    def p(self, w: str, h2: str, h1: str) -> float:
        """Linear-space probability (for linear interpolation of models)."""
        return 10.0 ** self.logp(w, h2, h1)

    def __len__(self) -> int:
        return len(self.uni)

    def stats(self) -> str:
        return (
            f"unigrams={len(self.uni):,} bigrams={len(self.bi):,} "
            f"trigrams={len(self.tri):,} tokens={self.total:,.0f}"
        )


class EmptyLM:
    """Null object: personal LM placeholder for the baseline config."""

    def p(self, w: str, h2: str, h1: str) -> float:
        return 0.0

    def logp(self, w: str, h2: str, h1: str) -> float:
        return -99.0
