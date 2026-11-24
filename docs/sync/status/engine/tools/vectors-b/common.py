"""Output helper shared by the generators: the same bytes on every run and locale."""
import json
import sys


def emit(obj):
    text = json.dumps(obj, indent=2, ensure_ascii=False) + "\n"
    sys.stdout.buffer.write(text.encode("utf-8"))
