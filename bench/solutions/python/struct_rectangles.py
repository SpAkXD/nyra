from dataclasses import dataclass


@dataclass
class Rect:
    width: int
    height: int


def area(rect):
    return rect.width * rect.height


def perimeter(rect):
    return 2 * (rect.width + rect.height)


for rect in (Rect(3, 4), Rect(10, 2), Rect(7, 7)):
    print(area(rect), perimeter(rect))
