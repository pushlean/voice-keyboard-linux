!#/bin/env bash

uv venv --python 3.12 --seed
source .venv/bin/activate

uv pip install -U vllm==0.19.0 --torch-backend=auto
uv pip install vllm[audio]
uv pip install librosa
