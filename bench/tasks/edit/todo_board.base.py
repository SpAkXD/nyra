"""A to-do board: reads commands from standard input, one per line.

    add PRIORITY TITLE...       add a task (PRIORITY 1 is the most urgent, 3 the least)
    done ID                     mark a task as done
    undo ID                     mark a done task as open again
    drop ID                     delete a task
    move ID PRIORITY            change the priority of a task
    list                        the open tasks, most urgent first
    all                         every task in the order it was added
    stats                       how many tasks are open and done
"""

import sys
from dataclasses import dataclass


@dataclass
class Todo:
    id: int
    title: str
    priority: int
    done: bool


@dataclass
class Board:
    todos: list
    next_id: int


def parse_number(text: str) -> int:
    """A whole number written with digits only, or -1."""
    if text == "" or not text.isascii() or not text.isdigit() or len(text) > 6:
        return -1
    return int(text)


def valid_priority(value: int) -> bool:
    return 1 <= value <= 3


def find_todo(board: Board, todo_id: int) -> int:
    for i, todo in enumerate(board.todos):
        if todo.id == todo_id:
            return i
    return -1


def describe(todo: Todo) -> str:
    return f"#{todo.id} [{todo.priority}] {todo.title}"


def cmd_add(board: Board, args: list) -> None:
    if len(args) < 3:
        print("error: usage: add PRIORITY TITLE")
        return
    priority = parse_number(args[1])
    if not valid_priority(priority):
        print("error: priority must be 1, 2 or 3")
        return
    title = " ".join(args[2:])
    board.todos.append(Todo(board.next_id, title, priority, False))
    print(f"added #{board.next_id}: {title} (priority {priority})")
    board.next_id += 1


def cmd_done(board: Board, args: list) -> None:
    if len(args) != 2:
        print("error: usage: done ID")
        return
    at = find_todo(board, parse_number(args[1]))
    if at < 0:
        print(f"error: no task {args[1]}")
        return
    if board.todos[at].done:
        print(f"error: #{board.todos[at].id} is already done")
        return
    board.todos[at].done = True
    print(f"completed #{board.todos[at].id}")


def cmd_undo(board: Board, args: list) -> None:
    if len(args) != 2:
        print("error: usage: undo ID")
        return
    at = find_todo(board, parse_number(args[1]))
    if at < 0:
        print(f"error: no task {args[1]}")
        return
    if not board.todos[at].done:
        print(f"error: #{board.todos[at].id} is not done")
        return
    board.todos[at].done = False
    print(f"reopened #{board.todos[at].id}")


def cmd_drop(board: Board, args: list) -> None:
    if len(args) != 2:
        print("error: usage: drop ID")
        return
    at = find_todo(board, parse_number(args[1]))
    if at < 0:
        print(f"error: no task {args[1]}")
        return
    print(f"dropped #{board.todos[at].id}")
    del board.todos[at]


def cmd_move(board: Board, args: list) -> None:
    if len(args) != 3:
        print("error: usage: move ID PRIORITY")
        return
    at = find_todo(board, parse_number(args[1]))
    if at < 0:
        print(f"error: no task {args[1]}")
        return
    priority = parse_number(args[2])
    if not valid_priority(priority):
        print("error: priority must be 1, 2 or 3")
        return
    board.todos[at].priority = priority
    print(f"#{board.todos[at].id} now has priority {priority}")


def cmd_list(board: Board, args: list) -> None:
    open_todos = [todo for todo in board.todos if not todo.done]
    if len(open_todos) == 0:
        print("nothing to do")
        return
    for todo in sorted(open_todos, key=lambda t: (t.priority, t.id)):
        print(describe(todo))


def cmd_all(board: Board, args: list) -> None:
    if len(board.todos) == 0:
        print("the board is empty")
        return
    for todo in board.todos:
        mark = "x" if todo.done else " "
        print(f"#{todo.id} [{mark}] [{todo.priority}] {todo.title}")


def cmd_stats(board: Board, args: list) -> None:
    done = len([todo for todo in board.todos if todo.done])
    print(f"open={len(board.todos) - done} done={done}")


def execute(board: Board, line: str) -> None:
    args = [word for word in line.split(" ") if word != ""]
    if len(args) == 0:
        return
    command = args[0]
    if command == "add":
        cmd_add(board, args)
    elif command == "done":
        cmd_done(board, args)
    elif command == "undo":
        cmd_undo(board, args)
    elif command == "drop":
        cmd_drop(board, args)
    elif command == "move":
        cmd_move(board, args)
    elif command == "list":
        cmd_list(board, args)
    elif command == "all":
        cmd_all(board, args)
    elif command == "stats":
        cmd_stats(board, args)
    else:
        print(f"error: unknown command {command}")


def main() -> None:
    board = Board(todos=[], next_id=1)
    for line in sys.stdin.read().splitlines():
        execute(board, line)


if __name__ == "__main__":
    main()
