import datetime
import re
import sys

SHAPE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")


def parse(token):
    if not SHAPE.fullmatch(token):
        return None
    try:
        return datetime.date(int(token[0:4]), int(token[5:7]), int(token[8:10]))
    except ValueError:
        return None


for line in sys.stdin.read().splitlines():
    parts = line.split(" ")
    if len(parts) != 2:
        print("invalid")
        continue
    a, b = parse(parts[0]), parse(parts[1])
    if a is None or b is None:
        print("invalid")
    else:
        print((b - a).days)
