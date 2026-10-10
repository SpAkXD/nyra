from dataclasses import dataclass

@dataclass
class Rect:
    width: int
    height: int

def area(r: Rect) -> int:
    return r.width * r.height

def perimeter(r: Rect) -> int:
    return 2 * (r.width + r.height)

def main():
    rects = [Rect(3, 4), Rect(10, 2), Rect(7, 7)]
    for r in rects:
        print(f"{area(r)} {perimeter(r)}")

if __name__ == "__main__":
    main()
