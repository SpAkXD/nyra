"""A small bank: reads commands from standard input, one per line. Amounts are written in dollars (`12`, `12.5`, `12.50`).

    open NAME                    open an account with a zero balance
    deposit NAME AMOUNT          put money on an account
    withdraw NAME AMOUNT         take money out (never below zero)
    transfer FROM TO AMOUNT      move money between two accounts
    balance NAME                 show one balance
    statement NAME               list the transactions of one account
    summary                      all accounts and the total
"""

import sys
from dataclasses import dataclass


@dataclass
class Account:
    name: str
    balance: int  # cents
    history: list


@dataclass
class Bank:
    accounts: list


def money(cents: int) -> str:
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}${cents // 100}.{cents % 100:02d}"


def parse_amount(text: str) -> int:
    """An amount of money with at most two decimals, in cents, or -1."""
    parts = text.split(".")
    if len(parts) > 2 or parts[0] == "" or not parts[0].isascii() or not parts[0].isdigit() or len(parts[0]) > 9:
        return -1
    cents = 0
    if len(parts) == 2:
        if len(parts[1]) < 1 or len(parts[1]) > 2 or not parts[1].isascii() or not parts[1].isdigit():
            return -1
        cents = int(parts[1]) * (10 if len(parts[1]) == 1 else 1)
    return int(parts[0]) * 100 + cents


def find_account(bank: Bank, name: str) -> int:
    for i, account in enumerate(bank.accounts):
        if account.name == name:
            return i
    return -1


def cmd_open(bank: Bank, args: list) -> None:
    if len(args) != 2:
        print("error: usage: open NAME")
        return
    if find_account(bank, args[1]) >= 0:
        print(f"error: {args[1]} already exists")
        return
    bank.accounts.append(Account(args[1], 0, []))
    print(f"opened {args[1]}")


def cmd_deposit(bank: Bank, args: list) -> None:
    if len(args) != 3:
        print("error: usage: deposit NAME AMOUNT")
        return
    amount = parse_amount(args[2])
    if amount <= 0:
        print("error: bad amount")
        return
    at = find_account(bank, args[1])
    if at < 0:
        print(f"error: unknown account {args[1]}")
        return
    account = bank.accounts[at]
    account.balance += amount
    account.history.append(f"deposit +{money(amount)}")
    print(f"deposited {money(amount)} to {account.name}, balance {money(account.balance)}")


def cmd_withdraw(bank: Bank, args: list) -> None:
    if len(args) != 3:
        print("error: usage: withdraw NAME AMOUNT")
        return
    amount = parse_amount(args[2])
    if amount <= 0:
        print("error: bad amount")
        return
    at = find_account(bank, args[1])
    if at < 0:
        print(f"error: unknown account {args[1]}")
        return
    account = bank.accounts[at]
    if account.balance < amount:
        print(f"error: insufficient funds in {account.name} (balance {money(account.balance)})")
        return
    account.balance -= amount
    account.history.append(f"withdraw -{money(amount)}")
    print(f"withdrew {money(amount)} from {account.name}, balance {money(account.balance)}")


def cmd_transfer(bank: Bank, args: list) -> None:
    if len(args) != 4:
        print("error: usage: transfer FROM TO AMOUNT")
        return
    amount = parse_amount(args[3])
    if amount <= 0:
        print("error: bad amount")
        return
    source = find_account(bank, args[1])
    target = find_account(bank, args[2])
    if source < 0:
        print(f"error: unknown account {args[1]}")
        return
    if target < 0:
        print(f"error: unknown account {args[2]}")
        return
    if source == target:
        print("error: cannot transfer to the same account")
        return
    if bank.accounts[source].balance < amount:
        print(f"error: insufficient funds in {args[1]} (balance {money(bank.accounts[source].balance)})")
        return
    bank.accounts[source].balance -= amount
    bank.accounts[target].balance += amount
    bank.accounts[source].history.append(f"transfer to {args[2]} -{money(amount)}")
    bank.accounts[target].history.append(f"transfer from {args[1]} +{money(amount)}")
    print(f"transferred {money(amount)} from {args[1]} to {args[2]}")


def cmd_balance(bank: Bank, args: list) -> None:
    if len(args) != 2:
        print("error: usage: balance NAME")
        return
    at = find_account(bank, args[1])
    if at < 0:
        print(f"error: unknown account {args[1]}")
        return
    print(f"{args[1]}: {money(bank.accounts[at].balance)}")


def cmd_statement(bank: Bank, args: list) -> None:
    if len(args) != 2:
        print("error: usage: statement NAME")
        return
    at = find_account(bank, args[1])
    if at < 0:
        print(f"error: unknown account {args[1]}")
        return
    account = bank.accounts[at]
    print(f"{account.name} statement:")
    if len(account.history) == 0:
        print("  (no transactions)")
    for number, entry in enumerate(account.history, 1):
        print(f"  {number}. {entry}")
    print(f"  balance {money(account.balance)}")


def cmd_summary(bank: Bank, args: list) -> None:
    if len(bank.accounts) == 0:
        print("no accounts")
        return
    total = 0
    for account in sorted(bank.accounts, key=lambda a: a.name):
        total += account.balance
        print(f"{account.name} {money(account.balance)}")
    print(f"accounts={len(bank.accounts)} total={money(total)}")


def execute(bank: Bank, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "open":
        cmd_open(bank, args)
    elif command == "deposit":
        cmd_deposit(bank, args)
    elif command == "withdraw":
        cmd_withdraw(bank, args)
    elif command == "transfer":
        cmd_transfer(bank, args)
    elif command == "balance":
        cmd_balance(bank, args)
    elif command == "statement":
        cmd_statement(bank, args)
    elif command == "summary":
        cmd_summary(bank, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    bank = Bank(accounts=[])
    for line in sys.stdin.read().splitlines():
        execute(bank, line)


if __name__ == "__main__":
    main()
