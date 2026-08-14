#!/usr/bin/env bash

mkdir -p "$HOME/.local/state/vllm"
mkdir -p "$HOME/scratch/vllm/cohere-transcribe"

cd "$HOME/scratch/vllm/cohere-transcribe"

if [[ ! -f .env ]]; then
    cat >.env <<'EOF'
HF_TOKEN=
EOF
fi

echo "Set HF_TOKEN in $PWD/.env before starting the service."
echo "Create a Hugging Face read token at https://huggingface.co/settings/tokens"
echo "with permissions 'Read contents of your repos' and 'Read contents of public gated repos you can access'"

uv venv --python 3.12 --seed
source .venv/bin/activate

uv pip install -U vllm==0.19.0 --torch-backend=auto
uv pip install vllm[audio]
uv pip install librosa
