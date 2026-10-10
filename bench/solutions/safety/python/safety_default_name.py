import os
import sys

line = sys.stdin.readline().strip()
if not line:
    line = os.environ.get("USER") or os.environ.get("USERNAME") or "stranger"
print(f"Hello, {line}!")
