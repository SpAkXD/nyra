from dataclasses import dataclass


@dataclass
class Rect:
    width: int
    height: int


for rect in (Rect(3, 4), Rect(10, 2), Rect(7, 7)):
    print(rect.width * rect.height, 2 * (rect.width + rect.height))
