#!/bin/bash
set -e

echo 'Synapse Mock Worker'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

pip install -r requirements.txt --quiet
python main.py

#!/bin/bash
set -e
echo 'Starting Synapse mock agent worker...'
pip install -r requirements.txt
python main.py
