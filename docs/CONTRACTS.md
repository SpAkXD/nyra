# Contracts

A contract says what a function needs and what it guarantees, in Nyra conditions:

```nyra
fn withdraw(balance: int, amount: int) -> int
    requires amount > 0, amount <= balance
    ensures result >= 0, result == balance - amount
= balance - amount

fn push_twice(inout xs: [str], s: str) {
    requires s.len() > 0
    ensures xs.len() == old(xs).len() + 2
    xs.push(s)
    xs.push(s)
}
```

- `requires`: conditions on the parameters. `ensures`: conditions after the call; `result` is the return value,
  `old(x)` the value of `inout` parameter `x` when the call began.
- One line per word, conditions separated by commas, as many lines as you like: between the signature and
  `= expr`, or as the first lines of a `{ }` body.

## What is checked when

| when | what | error |
|---|---|---|
| `nyra check` (also `run`, `build`, `test`) | `result`, `old(...)` used where they mean nothing | E0423 |
| `nyra check` (also `run`, `build`, `test`) | a searched input satisfies `requires` but breaks `ensures` | E0421 |
| `nyra check` (also `run`, `build`, `test`) | a searched input satisfies `requires` but the call stops with a runtime error | E0422 |
| every call, every backend | a `requires` condition is false: the caller is wrong | runtime E0420 |
| every call, every backend | an `ensures` condition is false: the function is wrong | runtime E0421 |

The counterexample search tries 300 inputs per function (`NYRA_CONTRACT_TRIES=N`; `0` turns it off): boundary
values (0, ±1, the int limits, empty and short strings and arrays, `none`, every enum variant), the constants of the
function and its contract with their neighbours, the literal arguments of its `ex` lines, then seeded random values.
It keeps the inputs that satisfy `requires`, runs the function in the interpreter (at most 3,000,000 steps per
function), and shrinks the first failure (ints toward 0, shorter strings and arrays) before reporting it:

```
error[E0421]: withdraw(1, 1) breaks ensures result > 0: result is 0
  --> bank.nyra:3:13
  = hint: a counterexample: `nyra check` called `withdraw(1, 1)`, which satisfies every `requires`, ...
```

The same program always gives the same counterexample. Finding none is no proof: the search is testing.

## Limits

- Not searched (still checked at every call): functions that use script variables, and functions that use `fs`,
  `os`, `input`, `time` or `random`, also through the functions they call.
- Generated values: floats are finite, strings have up to 8 characters (mostly ASCII), arrays up to 6 elements,
  maps up to 4 entries, values nest at most 3 levels. A call nested deeper than 10,000 is skipped.
- An overflow counts: `fn neg(x: int) -> int ensures result + x == 0 = -x` fails for the smallest int. Bound the
  input (`requires x > -1000000000`) when such values never occur.
- The checks cost a call and the conditions at run time; there is no switch to turn them off yet.