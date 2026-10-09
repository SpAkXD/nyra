import os

name = os.environ.get("USER") or os.environ.get("USERNAME") or "friend"
print(f"Hello, {name}!")
