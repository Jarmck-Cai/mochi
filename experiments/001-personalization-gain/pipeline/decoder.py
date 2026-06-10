"""Beam-search decoder over the key-stream lattice.

Scoring follows DESIGN.md section 3 (linear interpolation of a general LM,
a personal LM and a personal-lexicon prior).  The two LMs are combined in
probability space:

    P(w|h) = mu_g * P_general(w|h) + mu_p * P_personal(w|h)

which is the additive form of "通用LM + 个人LM 插值": the personal layer adds
probability mass where the user history knows better and costs nothing where
it is silent.  On top of that, log-space bonuses implement the personal
lexicon prior and per-arc-type priors (the 场景先验 slot of the formula is
left as a constant 0 for this offline experiment).
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from .lattice import (
    ARC_EN_GENERAL,
    ARC_EN_PERSONAL,
    ARC_FALLBACK,
    ARC_PY_CHAR,
    ARC_PY_PERSONAL,
    ARC_PY_WORD,
    Arc,
    LatticeBuilder,
)
from .lm import BOS, BackoffTrigramLM, EmptyLM

# Log10 prior per arc type: dictionary words are neutral; single chars get a
# mild penalty (prefer longer words); English arcs pay a mode-switch cost;
# fallback letters are last resort.
DEFAULT_ARC_PRIOR = {
    ARC_PY_WORD: 0.0,
    ARC_PY_PERSONAL: 0.0,
    ARC_PY_CHAR: -0.6,
    ARC_EN_GENERAL: -3.0,
    ARC_EN_PERSONAL: -1.0,
    ARC_FALLBACK: -12.0,
}


@dataclass
class ScorerConfig:
    mu_general: float = 1.0       # weight of general LM probability
    mu_personal: float = 0.0      # weight of personal LM probability (0 = off)
    lambda_lexicon: float = 0.0   # weight of personal-lexicon bonus (0 = off)
    arc_prior: dict | None = None

    def label(self) -> str:
        parts = []
        parts.append("generalLM")
        if self.mu_personal > 0:
            parts.append("personalLM")
        if self.lambda_lexicon > 0:
            parts.append("personalLexicon")
        return "+".join(parts)


class Scorer:
    def __init__(
        self,
        general_lm: BackoffTrigramLM,
        personal_lm: BackoffTrigramLM | EmptyLM,
        personal_lexicon: dict[str, float],
        config: ScorerConfig,
    ):
        self.general_lm = general_lm
        self.personal_lm = personal_lm if config.mu_personal > 0 else EmptyLM()
        self.config = config
        self.arc_prior = dict(DEFAULT_ARC_PRIOR)
        if config.arc_prior:
            self.arc_prior.update(config.arc_prior)
        # Pre-compute lexicon bonuses: log-count, capped, scaled later.
        self.lex_bonus: dict[str, float] = {}
        if config.lambda_lexicon > 0:
            for w, c in personal_lexicon.items():
                self.lex_bonus[w] = min(3.0, math.log10(1.0 + c) + 0.5)

    def arc_score(self, arc: Arc, h2: str, h1: str) -> float:
        cfg = self.config
        w = arc.lm_token  # lowercase for English; surface casing is UI-only
        p = cfg.mu_general * self.general_lm.p(w, h2, h1)
        if cfg.mu_personal > 0:
            p += cfg.mu_personal * self.personal_lm.p(w, h2, h1)
        score = math.log10(p) if p > 0 else -12.0
        score += self.arc_prior[arc.kind]
        if cfg.lambda_lexicon > 0:
            b = self.lex_bonus.get(w)
            if b:
                score += cfg.lambda_lexicon * b
        return score


@dataclass
class Hypothesis:
    score: float
    h2: str
    h1: str
    words: tuple[str, ...]


def decode(
    keys: str,
    lattice_builder: LatticeBuilder,
    scorer: Scorer,
    beam_width: int = 12,
) -> str:
    """Best-path beam search; returns the concatenated surface string."""
    n = len(keys)
    if n == 0:
        return ""
    arcs = lattice_builder.build(keys)
    # beams[pos] = best hypotheses ending exactly at pos, deduped by LM state.
    beams: list[dict[tuple[str, str], Hypothesis]] = [dict() for _ in range(n + 1)]
    beams[0][(BOS, BOS)] = Hypothesis(0.0, BOS, BOS, ())
    for pos in range(n):
        if not beams[pos]:
            continue
        frontier = sorted(beams[pos].values(), key=lambda h: -h.score)[:beam_width]
        for hyp in frontier:
            for arc in arcs[pos]:
                s = hyp.score + scorer.arc_score(arc, hyp.h2, hyp.h1)
                state = (hyp.h1, arc.lm_token)
                bucket = beams[arc.end]
                cur = bucket.get(state)
                if cur is None or s > cur.score:
                    bucket[state] = Hypothesis(
                        s, hyp.h1, arc.lm_token, hyp.words + (arc.word,)
                    )
    if not beams[n]:
        return keys  # unreachable in practice (fallback arcs guarantee a path)
    best = max(beams[n].values(), key=lambda h: h.score)
    return "".join(best.words)
