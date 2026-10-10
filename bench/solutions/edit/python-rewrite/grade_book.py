"""A grade book: reads commands from standard input, one per line.

    student NAME                  register a student
    score NAME COURSE POINTS      record a score (0 to 100) of a student in a course
    average NAME                  the average of a student's scores
    grade NAME                    the letter grade for that average
    courses NAME                  the average per course
    ranking                       the students from the best to the weakest average
    report                        every student with average and grade
"""

import sys
from dataclasses import dataclass


@dataclass
class Score:
    course: str
    points: int


@dataclass
class Student:
    name: str
    scores: list


@dataclass
class Book:
    students: list


def parse_points(text: str) -> int:
    """A whole number from 0 to 100 written with digits only, or -1."""
    if text == "" or not text.isascii() or not text.isdigit() or len(text) > 3:
        return -1
    value = int(text)
    return value if value <= 100 else -1


def find_student(book: Book, name: str) -> int:
    for i, student in enumerate(book.students):
        if student.name == name:
            return i
    return -1


def counted(student: Student) -> list:
    """The scores that count towards the student's average (with 4 or more scores the lowest one is dropped)."""
    scores = student.scores
    if len(scores) < 4:
        return scores
    lowest = 0
    for i in range(1, len(scores)):
        if scores[i].points < scores[lowest].points:
            lowest = i
    return scores[:lowest] + scores[lowest + 1:]


def total_points(student: Student) -> int:
    return sum(score.points for score in counted(student))


def average10(total: int, count: int) -> int:
    """The average in tenths, rounded half up."""
    return (total * 20 + count) // (2 * count)


def format_tenths(value: int) -> str:
    return f"{value // 10}.{value % 10}"


def letter(total: int, count: int) -> str:
    if total >= 92 * count:
        return "A"
    if total >= 84 * count:
        return "B"
    if total >= 74 * count:
        return "C"
    if total >= 62 * count:
        return "D"
    return "F"


def cmd_student(book: Book, args: list) -> None:
    if len(args) != 2:
        print("error: usage: student NAME")
        return
    if find_student(book, args[1]) >= 0:
        print(f"error: {args[1]} is already registered")
        return
    book.students.append(Student(args[1], []))
    print(f"registered {args[1]}")


def cmd_score(book: Book, args: list) -> None:
    if len(args) != 4:
        print("error: usage: score NAME COURSE POINTS")
        return
    at = find_student(book, args[1])
    if at < 0:
        print(f"error: unknown student {args[1]}")
        return
    points = parse_points(args[3])
    if points < 0:
        print("error: bad points")
        return
    book.students[at].scores.append(Score(args[2], points))
    print(f"recorded {points} for {args[1]} in {args[2]}")


def cmd_average(book: Book, args: list) -> None:
    if len(args) != 2:
        print("error: usage: average NAME")
        return
    at = find_student(book, args[1])
    if at < 0:
        print(f"error: unknown student {args[1]}")
        return
    student = book.students[at]
    count = len(counted(student))
    if count == 0:
        print(f"{student.name} has no scores")
        return
    print(f"{student.name}: {format_tenths(average10(total_points(student), count))} ({count} scores)")


def cmd_grade(book: Book, args: list) -> None:
    if len(args) != 2:
        print("error: usage: grade NAME")
        return
    at = find_student(book, args[1])
    if at < 0:
        print(f"error: unknown student {args[1]}")
        return
    student = book.students[at]
    count = len(counted(student))
    if count == 0:
        print(f"{student.name} has no scores")
        return
    print(f"{student.name}: {letter(total_points(student), count)}")


def cmd_courses(book: Book, args: list) -> None:
    if len(args) != 2:
        print("error: usage: courses NAME")
        return
    at = find_student(book, args[1])
    if at < 0:
        print(f"error: unknown student {args[1]}")
        return
    student = book.students[at]
    scores = counted(student)
    if len(scores) == 0:
        print(f"{student.name} has no scores")
        return
    print(f"{student.name} courses:")
    for course in sorted({score.course for score in scores}):
        points = [score.points for score in scores if score.course == course]
        print(f"  {course}: {format_tenths(average10(sum(points), len(points)))} ({len(points)})")


def cmd_ranking(book: Book, args: list) -> None:
    ranked = [s for s in book.students if len(counted(s)) > 0]
    if len(ranked) == 0:
        print("no ranking yet")
        return
    ranked.sort(key=lambda s: (-(total_points(s) * 1000000 // len(counted(s))), s.name))
    for place, student in enumerate(ranked, 1):
        count = len(counted(student))
        total = total_points(student)
        print(f"{place}. {student.name} {format_tenths(average10(total, count))} {letter(total, count)}")


def cmd_report(book: Book, args: list) -> None:
    if len(book.students) == 0:
        print("no students")
        return
    for student in sorted(book.students, key=lambda s: s.name):
        count = len(counted(student))
        if count == 0:
            print(f"{student.name} -")
        else:
            total = total_points(student)
            print(f"{student.name} {format_tenths(average10(total, count))} {letter(total, count)} ({count})")


def execute(book: Book, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "student":
        cmd_student(book, args)
    elif command == "score":
        cmd_score(book, args)
    elif command == "average":
        cmd_average(book, args)
    elif command == "grade":
        cmd_grade(book, args)
    elif command == "courses":
        cmd_courses(book, args)
    elif command == "ranking":
        cmd_ranking(book, args)
    elif command == "report":
        cmd_report(book, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    book = Book(students=[])
    for line in sys.stdin.read().splitlines():
        execute(book, line)


if __name__ == "__main__":
    main()
