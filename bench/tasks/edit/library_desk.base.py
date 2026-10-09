"""A library desk: reads commands from standard input, one per line. Days are whole numbers.

    book ID TITLE...             add a book
    member NAME                  register a member
    borrow NAME ID DAY           lend a book (it is due 14 days later)
    return NAME ID DAY           take a book back; late books cost a fine
    fines NAME                   what a member owes
    status                       every book and where it is
    late DAY                     the loans that are overdue on a day
"""

import sys
from dataclasses import dataclass

LOAN_DAYS = 14


@dataclass
class Book:
    id: str
    title: str
    holder: str  # the member who has it, or ""
    due: int


@dataclass
class Member:
    name: str
    fines: int  # cents


@dataclass
class Library:
    books: list
    members: list


def money(cents: int) -> str:
    return f"${cents // 100}.{cents % 100:02d}"


def parse_day(text: str) -> int:
    """A day number written with digits only, or -1."""
    if text == "" or not text.isascii() or not text.isdigit() or len(text) > 6:
        return -1
    return int(text)


def find_book(library: Library, book_id: str) -> int:
    for i, book in enumerate(library.books):
        if book.id == book_id:
            return i
    return -1


def find_member(library: Library, name: str) -> int:
    for i, member in enumerate(library.members):
        if member.name == name:
            return i
    return -1


def fine_for(days_late: int) -> int:
    """The fine in cents for a book that comes back `days_late` days after its due day."""
    return days_late * 10


def cmd_book(library: Library, args: list) -> None:
    if len(args) < 3:
        print("error: usage: book ID TITLE")
        return
    if find_book(library, args[1]) >= 0:
        print(f"error: book {args[1]} already exists")
        return
    title = " ".join(args[2:])
    library.books.append(Book(args[1], title, "", 0))
    print(f"added book {args[1]}: {title}")


def cmd_member(library: Library, args: list) -> None:
    if len(args) != 2:
        print("error: usage: member NAME")
        return
    if find_member(library, args[1]) >= 0:
        print(f"error: {args[1]} is already a member")
        return
    library.members.append(Member(args[1], 0))
    print(f"welcome {args[1]}")


def cmd_borrow(library: Library, args: list) -> None:
    if len(args) != 4:
        print("error: usage: borrow NAME ID DAY")
        return
    day = parse_day(args[3])
    if day < 0:
        print("error: bad day")
        return
    if find_member(library, args[1]) < 0:
        print(f"error: unknown member {args[1]}")
        return
    at = find_book(library, args[2])
    if at < 0:
        print(f"error: unknown book {args[2]}")
        return
    book = library.books[at]
    if book.holder != "":
        print(f"error: {book.id} is already on loan")
        return
    book.holder = args[1]
    book.due = day + LOAN_DAYS
    print(f"{args[1]} borrowed {book.id}, due day {book.due}")


def cmd_return(library: Library, args: list) -> None:
    if len(args) != 4:
        print("error: usage: return NAME ID DAY")
        return
    day = parse_day(args[3])
    if day < 0:
        print("error: bad day")
        return
    at = find_book(library, args[2])
    if at < 0:
        print(f"error: unknown book {args[2]}")
        return
    book = library.books[at]
    if book.holder != args[1]:
        print(f"error: {args[1]} does not have {book.id}")
        return
    book.holder = ""
    if day <= book.due:
        print(f"{args[1]} returned {book.id} on time")
        return
    days_late = day - book.due
    fine = fine_for(days_late)
    library.members[find_member(library, args[1])].fines += fine
    print(f"{args[1]} returned {book.id} {days_late} days late, fine {money(fine)}")


def cmd_fines(library: Library, args: list) -> None:
    if len(args) != 2:
        print("error: usage: fines NAME")
        return
    at = find_member(library, args[1])
    if at < 0:
        print(f"error: unknown member {args[1]}")
        return
    print(f"{args[1]} owes {money(library.members[at].fines)}")


def cmd_status(library: Library, args: list) -> None:
    if len(library.books) == 0:
        print("no books")
        return
    for book in sorted(library.books, key=lambda b: b.id):
        if book.holder == "":
            print(f"{book.id} {book.title}: available")
        else:
            print(f"{book.id} {book.title}: on loan to {book.holder} (due day {book.due})")


def cmd_late(library: Library, args: list) -> None:
    if len(args) != 2:
        print("error: usage: late DAY")
        return
    day = parse_day(args[1])
    if day < 0:
        print("error: bad day")
        return
    overdue = [b for b in library.books if b.holder != "" and b.due < day]
    if len(overdue) == 0:
        print("nothing is overdue")
        return
    for book in sorted(overdue, key=lambda b: (b.holder, b.id)):
        print(f"{book.holder} {book.id} due {book.due} ({day - book.due} days late)")


def execute(library: Library, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "book":
        cmd_book(library, args)
    elif command == "member":
        cmd_member(library, args)
    elif command == "borrow":
        cmd_borrow(library, args)
    elif command == "return":
        cmd_return(library, args)
    elif command == "fines":
        cmd_fines(library, args)
    elif command == "status":
        cmd_status(library, args)
    elif command == "late":
        cmd_late(library, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    library = Library(books=[], members=[])
    for line in sys.stdin.read().splitlines():
        execute(library, line)


if __name__ == "__main__":
    main()
