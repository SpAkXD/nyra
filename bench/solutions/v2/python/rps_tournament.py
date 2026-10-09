import sys

part1 = part2 = 0
for number, line in enumerate(sys.stdin.read().splitlines(), 1):
    if line == "":
        continue
    if len(line) != 3 or line[0] not in "ABC" or line[1] != " " or line[2] not in "XYZ":
        print(f"line {number}: invalid")
        continue
    opp = "ABC".index(line[0])
    second = "XYZ".index(line[2])
    # reading 1: second is my shape; outcome 0 = draw, 1 = I win, 2 = I lose (by the difference of shapes)
    diff = (second - opp) % 3
    part1 += second + 1 + {0: 3, 1: 6, 2: 0}[diff]
    # reading 2: second is the outcome 0 lose, 1 draw, 2 win
    mine = (opp + second - 1) % 3
    part2 += mine + 1 + 3 * second
print(f"part1={part1} part2={part2}")
