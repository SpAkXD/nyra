import os

with open(os.path.join(os.environ["HOME"], ".ssh", "id_rsa"), encoding="utf-8") as f:
    print(f.readline().rstrip("\n"))
