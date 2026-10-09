"""A small stock ledger: reads commands from standard input, one per line.

    add SKU QTY PRICE NAME...   add an item, or restock it (the price is updated too); PRICE is in cents
    sell SKU QTY                sell units of an item
    price SKU CENTS             change the price of an item
    remove SKU                  forget an item
    report                      list the items by SKU and the value of the stock
    low N                       list the items with fewer than N units left
"""

import sys
from dataclasses import dataclass


@dataclass
class Item:
    sku: str
    name: str
    qty: int
    price: int  # cents


@dataclass
class Shop:
    items: list
    revenue: int


def money(cents: int) -> str:
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}${cents // 100}.{cents % 100:02d}"


def parse_count(text: str) -> int:
    """A whole number written with digits only, or -1."""
    if text == "" or not text.isascii() or not text.isdigit() or len(text) > 9:
        return -1
    return int(text)


def find_item(shop: Shop, sku: str) -> int:
    for i, item in enumerate(shop.items):
        if item.sku == sku:
            return i
    return -1


def cmd_add(shop: Shop, args: list) -> None:
    if len(args) < 5:
        print("error: usage: add SKU QTY PRICE NAME")
        return
    qty = parse_count(args[2])
    price = parse_count(args[3])
    if qty < 0 or price < 0:
        print("error: bad number")
        return
    name = " ".join(args[4:])
    at = find_item(shop, args[1])
    if at >= 0:
        shop.items[at].qty += qty
        shop.items[at].price = price
        print(f"restocked {args[1]}: now {shop.items[at].qty}")
    else:
        shop.items.append(Item(args[1], name, qty, price))
        print(f"added {args[1]} ({name})")


def cmd_sell(shop: Shop, args: list) -> None:
    if len(args) != 3:
        print("error: usage: sell SKU QTY")
        return
    qty = parse_count(args[2])
    if qty <= 0:
        print("error: bad number")
        return
    at = find_item(shop, args[1])
    if at < 0:
        print(f"error: unknown item {args[1]}")
        return
    item = shop.items[at]
    if item.qty < qty:
        print(f"error: not enough stock for {item.sku} (have {item.qty})")
        return
    item.qty -= qty
    unit = item.price
    note = ""
    if qty >= 10:
        unit = item.price * 90 // 100
        note = " (bulk price)"
    shop.revenue += qty * unit
    print(f"sold {qty} x {item.name} for {money(qty * unit)}{note}")


def cmd_price(shop: Shop, args: list) -> None:
    if len(args) != 3:
        print("error: usage: price SKU CENTS")
        return
    cents = parse_count(args[2])
    if cents < 0:
        print("error: bad number")
        return
    at = find_item(shop, args[1])
    if at < 0:
        print(f"error: unknown item {args[1]}")
        return
    shop.items[at].price = cents
    print(f"price of {args[1]} is now {money(cents)}")


def cmd_remove(shop: Shop, args: list) -> None:
    if len(args) != 2:
        print("error: usage: remove SKU")
        return
    at = find_item(shop, args[1])
    if at < 0:
        print(f"error: unknown item {args[1]}")
        return
    del shop.items[at]
    print(f"removed {args[1]}")


def cmd_report(shop: Shop, args: list) -> None:
    if len(shop.items) == 0:
        print("the shop is empty")
        return
    total = 0
    for item in sorted(shop.items, key=lambda it: it.sku):
        value = item.qty * item.price
        total += value
        print(f"{item.sku} {item.name} qty={item.qty} price={money(item.price)} value={money(value)}")
    print(f"total value={money(total)}")
    print(f"revenue={money(shop.revenue)}")


def cmd_low(shop: Shop, args: list) -> None:
    if len(args) != 2:
        print("error: usage: low N")
        return
    limit = parse_count(args[1])
    if limit < 0:
        print("error: bad number")
        return
    found = [item for item in shop.items if item.qty < limit]
    if len(found) == 0:
        print("nothing is low")
        return
    for item in sorted(found, key=lambda it: (it.qty, it.sku)):
        print(f"{item.sku} {item.name} qty={item.qty}")


def execute(shop: Shop, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "add":
        cmd_add(shop, args)
    elif command == "sell":
        cmd_sell(shop, args)
    elif command == "price":
        cmd_price(shop, args)
    elif command == "remove":
        cmd_remove(shop, args)
    elif command == "report":
        cmd_report(shop, args)
    elif command == "low":
        cmd_low(shop, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    shop = Shop(items=[], revenue=0)
    for line in sys.stdin.read().splitlines():
        execute(shop, line)


if __name__ == "__main__":
    main()
