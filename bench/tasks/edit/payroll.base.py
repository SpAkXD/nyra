"""A small payroll: reads commands from standard input, one per line. Wages are in cents per hour.

    hire NAME WAGE               hire someone
    hours NAME H                 add hours worked in the current pay period
    raise NAME PERCENT           raise someone's wage by a percentage (rounded down to whole cents)
    fire NAME                    remove someone
    slip NAME                    the pay slip of one person
    payroll                      all pay slips, the total and the top earner
    budget CENTS                 does the payroll fit in a budget?
    reset                        start a new pay period (hours go back to zero)
"""

import sys
from dataclasses import dataclass


@dataclass
class Employee:
    name: str
    rate: int
    hours: int


@dataclass
class Staff:
    people: list


def money(cents: int) -> str:
    return f"${cents // 100}.{cents % 100:02d}"


def parse_whole(text: str) -> int:
    """A whole number written with digits only, or -1."""
    if text == "" or not text.isascii() or not text.isdigit() or len(text) > 9:
        return -1
    return int(text)


def find_person(staff: Staff, name: str) -> int:
    for i, person in enumerate(staff.people):
        if person.name == name:
            return i
    return -1


def gross_pay(person: Employee) -> int:
    return person.rate * person.hours


def slip_line(person: Employee) -> str:
    return f"{person.name}: {person.hours} hours at {money(person.rate)}/h = {money(gross_pay(person))}"


def cmd_hire(staff: Staff, args: list) -> None:
    if len(args) != 3:
        print("error: usage: hire NAME WAGE")
        return
    wage = parse_whole(args[2])
    if wage <= 0:
        print("error: bad wage")
        return
    if find_person(staff, args[1]) >= 0:
        print(f"error: {args[1]} already works here")
        return
    staff.people.append(Employee(args[1], wage, 0))
    print(f"hired {args[1]} at {money(wage)}/h")


def cmd_hours(staff: Staff, args: list) -> None:
    if len(args) != 3:
        print("error: usage: hours NAME H")
        return
    hours = parse_whole(args[2])
    if hours <= 0 or hours > 168:
        print("error: bad hours")
        return
    at = find_person(staff, args[1])
    if at < 0:
        print(f"error: unknown person {args[1]}")
        return
    staff.people[at].hours += hours
    print(f"{args[1]} worked {hours} hours (total {staff.people[at].hours})")


def cmd_raise(staff: Staff, args: list) -> None:
    if len(args) != 3:
        print("error: usage: raise NAME PERCENT")
        return
    percent = parse_whole(args[2])
    if percent <= 0:
        print("error: bad percentage")
        return
    at = find_person(staff, args[1])
    if at < 0:
        print(f"error: unknown person {args[1]}")
        return
    staff.people[at].rate = staff.people[at].rate * (100 + percent) // 100
    print(f"{args[1]} now earns {money(staff.people[at].rate)}/h")


def cmd_fire(staff: Staff, args: list) -> None:
    if len(args) != 2:
        print("error: usage: fire NAME")
        return
    at = find_person(staff, args[1])
    if at < 0:
        print(f"error: unknown person {args[1]}")
        return
    del staff.people[at]
    print(f"fired {args[1]}")


def cmd_slip(staff: Staff, args: list) -> None:
    if len(args) != 2:
        print("error: usage: slip NAME")
        return
    at = find_person(staff, args[1])
    if at < 0:
        print(f"error: unknown person {args[1]}")
        return
    print(slip_line(staff.people[at]))


def cmd_payroll(staff: Staff, args: list) -> None:
    if len(staff.people) == 0:
        print("nobody works here")
        return
    total = 0
    best = None
    for person in sorted(staff.people, key=lambda p: p.name):
        print(slip_line(person))
        total += gross_pay(person)
        if best is None or gross_pay(person) > gross_pay(best):
            best = person
    print(f"total payroll {money(total)}")
    if best is not None and gross_pay(best) > 0:
        print(f"top earner: {best.name} ({money(gross_pay(best))})")


def cmd_budget(staff: Staff, args: list) -> None:
    if len(args) != 2:
        print("error: usage: budget CENTS")
        return
    budget = parse_whole(args[1])
    if budget < 0:
        print("error: bad budget")
        return
    total = sum(gross_pay(person) for person in staff.people)
    if total <= budget:
        print(f"within budget, {money(budget - total)} left")
    else:
        print(f"over budget by {money(total - budget)}")


def cmd_reset(staff: Staff, args: list) -> None:
    for person in staff.people:
        person.hours = 0
    print("new pay period")


def execute(staff: Staff, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "hire":
        cmd_hire(staff, args)
    elif command == "hours":
        cmd_hours(staff, args)
    elif command == "raise":
        cmd_raise(staff, args)
    elif command == "fire":
        cmd_fire(staff, args)
    elif command == "slip":
        cmd_slip(staff, args)
    elif command == "payroll":
        cmd_payroll(staff, args)
    elif command == "budget":
        cmd_budget(staff, args)
    elif command == "reset":
        cmd_reset(staff, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    staff = Staff(people=[])
    for line in sys.stdin.read().splitlines():
        execute(staff, line)


if __name__ == "__main__":
    main()
