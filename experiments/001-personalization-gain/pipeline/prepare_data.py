"""Download the open resources used by this experiment into data/.

Idempotent: existing files are skipped.  Run as:

    .venv/Scripts/python -m pipeline.prepare_data
"""

from __future__ import annotations

import io
import sys
import urllib.request
import zipfile
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"

RIME_DICT_URL = (
    "https://raw.githubusercontent.com/rime/rime-pinyin-simp/master/pinyin_simp.dict.yaml"
)
ICWB2_URL = "http://sighan.cs.uchicago.edu/bakeoff2005/data/icwb2-data.zip"
EN10K_URL = (
    "https://raw.githubusercontent.com/first20hours/google-10000-english/master/"
    "google-10000-english.txt"
)
D2L_BASE = "https://raw.githubusercontent.com/d2l-ai/d2l-zh/master/"
# Chinese chapters of "Dive into Deep Learning" in curriculum order, used as
# the STAND-IN personal corpus (domain-heavy Chinese with English terms).
D2L_CHAPTERS = [
    "chapter_preliminaries/ndarray.md",
    "chapter_preliminaries/autograd.md",
    "chapter_preliminaries/linear-algebra.md",
    "chapter_linear-networks/linear-regression.md",
    "chapter_linear-networks/softmax-regression.md",
    "chapter_multilayer-perceptrons/mlp.md",
    "chapter_multilayer-perceptrons/dropout.md",
    "chapter_multilayer-perceptrons/underfit-overfit.md",
    "chapter_multilayer-perceptrons/backprop.md",
    "chapter_deep-learning-computation/model-construction.md",
    "chapter_deep-learning-computation/parameters.md",
    "chapter_deep-learning-computation/use-gpu.md",
    "chapter_convolutional-neural-networks/why-conv.md",
    "chapter_convolutional-neural-networks/conv-layer.md",
    "chapter_convolutional-neural-networks/pooling.md",
    "chapter_convolutional-neural-networks/lenet.md",
    "chapter_convolutional-modern/alexnet.md",
    "chapter_convolutional-modern/vgg.md",
    "chapter_convolutional-modern/batch-norm.md",
    "chapter_convolutional-modern/resnet.md",
    "chapter_recurrent-neural-networks/rnn.md",
    "chapter_recurrent-neural-networks/sequence.md",
    "chapter_recurrent-modern/gru.md",
    "chapter_recurrent-modern/lstm.md",
    "chapter_recurrent-modern/seq2seq.md",
    "chapter_attention-mechanisms/attention-scoring-functions.md",
    "chapter_attention-mechanisms/multihead-attention.md",
    "chapter_attention-mechanisms/transformer.md",
    "chapter_optimization/optimization-intro.md",
    "chapter_optimization/gd.md",
    "chapter_optimization/sgd.md",
    "chapter_optimization/adam.md",
    "chapter_computer-vision/fine-tuning.md",
    "chapter_computer-vision/image-augmentation.md",
    "chapter_natural-language-processing-pretraining/word2vec.md",
    "chapter_natural-language-processing-pretraining/bert.md",
]


def fetch(url: str, dest: Path) -> None:
    if dest.exists() and dest.stat().st_size > 0:
        print(f"skip   {dest.name} (exists)")
        return
    print(f"fetch  {url}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url, timeout=60) as r:
        dest.write_bytes(r.read())


def main() -> int:
    fetch(RIME_DICT_URL, DATA_DIR / "dict" / "pinyin_simp.dict.yaml")
    fetch(EN10K_URL, DATA_DIR / "english" / "google-10000-english.txt")

    pku = DATA_DIR / "general" / "pku_training.utf8"
    msr = DATA_DIR / "general" / "msr_training.utf8"
    if not (pku.exists() and msr.exists()):
        print(f"fetch  {ICWB2_URL} (~50MB)")
        with urllib.request.urlopen(ICWB2_URL, timeout=300) as r:
            blob = r.read()
        with zipfile.ZipFile(io.BytesIO(blob)) as z:
            for name, dest in [
                ("icwb2-data/training/pku_training.utf8", pku),
                ("icwb2-data/training/msr_training.utf8", msr),
            ]:
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(z.read(name))
    else:
        print("skip   general corpus (exists)")

    standin = DATA_DIR / "personal_standin"
    standin.mkdir(parents=True, exist_ok=True)
    for i, chapter in enumerate(D2L_CHAPTERS, 1):
        name = f"{i:02d}_{chapter.rsplit('/', 1)[-1]}"
        fetch(D2L_BASE + chapter, standin / name)

    (DATA_DIR / "personal").mkdir(parents=True, exist_ok=True)
    print("done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
