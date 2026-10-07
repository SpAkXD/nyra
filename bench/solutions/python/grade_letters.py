def grade(score):
    if score >= 90:
        return "A"
    if score >= 80:
        return "B"
    if score >= 70:
        return "C"
    if score >= 60:
        return "D"
    return "F"


for score in (95, 90, 89, 75, 60, 59, 0):
    print(grade(score))
