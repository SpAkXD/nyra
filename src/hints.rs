//! What people (and AI models) usually mean when they write something Nyra does not have.
//! The lexer, parser and checker use these tables to turn "unknown thing" errors into a fix
//! that can be applied at once: other languages' words, symbols and type names, mapped to
//! the Nyra way of saying the same thing.

use crate::ast::Type;
use crate::diag::suggest;

/// A word from another language that Nyra spells differently or does not have.
/// Used where a syntax error is caused by that word (`a and b`, `elif`).
pub fn word(w: &str) -> Option<String> {
    let hint = match w {
        "elif" | "elsif" | "elseif" => "write `else if` (two words) to test another condition",
        "switch" | "case" => "Nyra has no `switch`: write `match value { 1 => ..., _ => ... }`, or chain `if` / `else if`",
        "and" => "write `&&` for logical and: `a && b`",
        "or" => "write `||` for logical or: `a || b`",
        "not" => "write `!` for logical not: `!done`",
        "then" => "Nyra has no `then`: the body of an `if` is `{ ... }` on the same line as the `if`",
        "null" | "nil" | "None" | "NULL" | "undefined" => {
            "Nyra has no null: a value that may be missing has an optional type `int?`, and its empty value is `none`: `var best: int? = none`"
        }
        "True" | "False" => "write `true` or `false` in lowercase",
        "mut" => "write `var` for a variable that changes: `var x = 0`",
        "const" | "static" | "final" => {
            "write `let` for a value that never changes and `var` for one that does (a constant for the whole program is a function: `fn limit() -> int = 100`)"
        }
        "function" | "func" | "fun" | "def" => "functions start with `fn`: `fn name(a: int) -> int { ... }`",
        "try" | "catch" | "throw" | "finally" | "except" | "raise" => {
            "Nyra has no exceptions: a failing operation (such as a division by zero or an index out of bounds) stops the program with a runtime error"
        }
        "self" | "this" => {
            "Nyra has no `self` or `this`: write a function that takes the value as a parameter, e.g. `fn area(r: Rect) -> int`"
        }
        "loop" | "do" | "repeat" | "until" | "foreach" => {
            "loops are `while cond { ... }`, `for i in 0..n { ... }` and `for x in xs { ... }` (for an endless loop write `while true { ... }` and leave it with `break`)"
        }
        "as" => "Nyra has no `as` casts: convert with `int(x)`, `float(x)`, `str(x)` or `char(n)`",
        "new" => "Nyra has no `new`: build a struct by calling its name with the fields, e.g. `Point(x: 1, y: 2)`",
        "println" | "printf" | "puts" | "echo" | "writeln" => "print with `print(x)`: it takes one value and ends the line",
        _ => return None,
    };
    Some(hint.to_string())
}

/// A word that starts a top-level item in another language.
pub fn top_level_word(w: &str) -> Option<String> {
    let hint = match w {
        "class" | "object" | "record" | "data" => {
            "Nyra has no classes: declare the data with `struct Name { field: int }` and write functions that take it, e.g. `fn area(r: Rect) -> int`"
        }
        "impl" | "trait" | "interface" | "protocol" | "extension" => {
            return Some(format!(
                "Nyra has no `{w}`: methods are plain functions that take the struct as a parameter, e.g. `fn area(r: Rect) -> int`"
            ))
        }
        "union" | "type" | "typedef" => {
            return Some(format!(
                "Nyra has no `{w}`: the top-level items are `fn`, `struct` and `enum` (`enum Dir {{ N, E, S, W }}`)"
            ))
        }
        "import" | "use" | "require" | "include" | "from" | "package" | "module" | "namespace" | "mod" => {
            "a program is one file; it imports standard modules with `use name` on a line of its own, e.g. `use math`"
        }
        "pub" | "public" | "private" | "protected" | "extern" | "export" => {
            "Nyra has no visibility modifiers: start the definition with `fn` or `struct`"
        }
        "const" | "static" | "final" => {
            "Nyra has no `const` or `static`: a `let` at the top level of a script is a constant every function can read (`let limit = 100`)"
        }
        _ => return word(w),
    };
    Some(hint.to_string())
}

/// Hint for an undefined variable whose name is a keyword or literal of another language.
pub fn undefined_variable(w: &str) -> Option<String> {
    match w {
        "null" | "nil" | "None" | "NULL" | "undefined" | "True" | "False" | "self" | "this" => word(w),
        _ => None,
    }
}

/// Hint for an undefined name used before a `.`: a library object of another language
/// (`console.log(x)`, `Math.sqrt(x)`, `fmt.Println(x)`).
pub fn receiver(w: &str) -> Option<String> {
    let hint = match w {
        "console" | "fmt" | "System" | "sys" | "io" | "Console" | "std" | "process" => {
            "print with `print(x)`: it takes one value and ends the line"
        }
        "Math" | "math" => "the math functions are in the standard module `math`: add `use math` and call `math.sqrt(x)`",
        "Integer" | "Number" | "Float" | "Double" | "String" | "Str" | "Char" | "Character" => {
            "convert with the builtins `int(x)`, `float(x)`, `str(x)` and `char(n)`; methods belong to values, e.g. `s.len()`"
        }
        "Array" | "List" | "Vec" | "list" | "array" => {
            "arrays are written `[1, 2, 3]` and their methods are called on a value, e.g. `xs.push(4)`"
        }
        "random" | "Random" | "rand" | "time" | "Date" | "os" | "fs" | "File" => {
            "use the standard modules: `use random` (`random.range(1, 7)`), `use time` (`time.now_ms()`), `use fs` (`fs.read(path)`), `use os` (`os.args()`)"
        }
        _ => return None,
    };
    Some(hint.to_string())
}

/// Hint for a call to a function that does not exist, but that other languages have.
pub fn undefined_function(w: &str) -> Option<String> {
    let hint = match w {
        "abs" => "Nyra has no `abs`: define it, e.g. `fn abs(x: int) -> int = if x < 0 { -x } else { x }`",
        "min" => "Nyra has no `min`: define it, e.g. `fn min(a: int, b: int) -> int = if a < b { a } else { b }`",
        "max" => "Nyra has no `max`: define it, e.g. `fn max(a: int, b: int) -> int = if a > b { a } else { b }`",
        "pow" | "power" | "powi" => "`pow` is in the standard module `math`: add `use math` and call `math.pow(x, y)`",
        "sqrt" | "cbrt" | "exp" | "log" | "sin" | "cos" | "tan" => {
            "the math functions are in the standard module `math`: add `use math` and call e.g. `math.sqrt(x)`"
        }
        "floor" | "ceil" | "round" | "trunc" => {
            "these are in the standard module `math` (`math.floor(x)` gives a float); `int(x)` truncates a float to an int"
        }
        "len" | "length" | "size" | "count" => "the length is a method: `xs.len()` or `s.len()`",
        "string" | "String" | "to_string" | "toString" | "tostring" | "format" | "itoa" | "repr" | "sprintf" => {
            "convert with `str(x)`, or build text with interpolation, e.g. `\"{x}\"`"
        }
        "parseInt" | "atoi" | "parse_int" | "Number" | "Integer" => "parse text with `int(s)` (a runtime error if it is not a number)",
        "parseFloat" | "atof" | "parse_float" | "Float" | "Double" => {
            "parse text with `float(s)` (a runtime error if it is not a number)"
        }
        "chr" | "fromCharCode" | "char_from" => "turn a code into a character with `char(n)`",
        "ord" | "charCodeAt" | "codePointAt" => "a character's code is `c.code()`; all codes of a string: `s.codes()`",
        "sorted" | "sort" => "sort an array in place with `xs.sort()`, or by a key with `xs.sort_by(x => key)`",
        "reversed" | "reverse" => "`xs.reversed()` gives a reversed copy of an array or a string; `xs.reverse()` reverses an array in place",
        "enumerate" => "for the position and the element write `for i, x in xs { ... }`",
        "sum" | "any" | "all" => return Some(format!("`{w}` is a method: `xs.{w}()`{}", if w == "sum" { "" } else { " with a test, e.g. `xs.any(x => x > 0)`" })),
        "map" | "filter" => return Some(format!("`{w}` is a method that takes a lambda: `xs.{w}(x => ...)`, or write a comprehension: `[x * 2 for x in xs if x > 0]`")),
        "reduce" | "fold" => "`xs.fold(start, (acc, x) => ...)` combines the elements, e.g. `xs.fold(0, (acc, x) => acc + x)`",
        "split" | "join" | "trim" | "strip" | "upper" | "lower" | "replace" | "contains" | "startswith" | "endswith"
        | "push" | "append" | "pop" | "insert" | "remove" => {
            return Some(format!("`{w}` is a method: call it on the value, e.g. `x.{}(...)`", method_spelling(w)))
        }
        "println" | "printf" | "puts" | "echo" | "writeln" => "print with `print(x)`: it takes one value and ends the line",
        "input" | "readline" | "read_line" | "scanf" | "gets" | "getline" => {
            "standard input is the module `input`: add `use input` and call `input.line()`"
        }
        "exit" | "quit" => "add `use os` and call `os.exit(code)`",
        "panic" | "assert" | "abort" => {
            "Nyra has no `panic` or `assert`: print a message and stop with `os.exit(1)` (add `use os`)"
        }
        "range" => "a range is written `a..b` in a loop: `for i in 0..10 { ... }`",
        "bool" => "Nyra has no `bool(x)`: compare instead, e.g. `x != 0`",
        "random" | "rand" | "randint" | "clock" | "sleep" => {
            "use the standard modules: `use random` (`random.range(1, 7)`, `random.random()`), `use time` (`time.mono_ms()`, `time.sleep_ms(ms)`)"
        }
        _ => return None,
    };
    Some(hint.to_string())
}

/// The Nyra method for a function name of another language (`startswith` is `starts_with`).
fn method_spelling(w: &str) -> &str {
    match w {
        "strip" => "trim",
        "startswith" => "starts_with",
        "endswith" => "ends_with",
        "append" => "push",
        other => other,
    }
}

/// Hint for a method that the type of `recv` does not have, but that other languages do:
/// `xs.length()` is `xs.len()`, `s.toUpperCase()` is `s.upper()`. `recv` is the receiver as the
/// program wrote it. Capital letters and underscores do not count (`startsWith` is `starts_with`).
pub fn method(t: Type, name: &str, recv: &str) -> Option<String> {
    let w: String = name.chars().filter(|c| *c != '_').map(|c| c.to_ascii_lowercase()).collect();
    let r = recv;
    let own = match t {
        Type::Array(_) => array_method(&w, r),
        Type::Str => str_method(&w, r),
        Type::Char => char_method(&w, r),
        _ => None,
    };
    own.or_else(|| match w.as_str() {
        "clone" | "copy" | "duplicate" => {
            Some(format!("values are copied when they are assigned: `var other = {r}` is already independent of `{r}`"))
        }
        "tostring" | "tostr" | "asstring" => Some(format!("`str({r})` gives the text that `print({r})` shows")),
        "equals" | "isequal" => Some(format!("compare with `==`: `{r} == other`")),
        "abs" | "min" | "max" | "pow" | "powi" | "sqrt" | "cbrt" | "exp" | "log" | "sin" | "cos" | "tan" | "floor" | "ceil"
        | "round" | "trunc"
            if matches!(t, Type::Int | Type::Float) =>
        {
            undefined_function(name)
        }
        _ => None,
    })
}

fn array_method(w: &str, r: &str) -> Option<String> {
    Some(match w {
        "length" | "size" => format!("the length is `{r}.len()`"),
        "isempty" | "empty" => format!("compare the length: `{r}.len() == 0`"),
        "append" | "add" | "pushback" | "addlast" => format!("add an element at the end with `{r}.push(x)` (`{r}` must be a `var`)"),
        "popback" | "poplast" | "removelast" => format!("`{r}.pop()` removes the last element and gives it back"),
        "shift" | "popfront" | "removefirst" => format!("`{r}.remove(0)` removes the first element and gives it back"),
        "unshift" | "pushfront" | "addfirst" => format!("`{r}.insert(0, x)` adds an element at the front"),
        "includes" | "has" | "contain" => format!("write `{r}.contains(x)`"),
        "indexof" | "find" | "position" => format!("`{r}.index_of(x)` gives the position of the first match, or -1"),
        "removeat" | "delete" | "erase" => format!("`{r}.remove(i)` removes the element at position `i` and gives it back"),
        "first" | "front" => format!(
            "the first element is `{r}[0]` (check `{r}.len() > 0` first); the first that passes a test: `{r}.find_index(x => test)`"
        ),
        "last" | "back" => format!("the last element is `{r}[{r}.len() - 1]` (check `{r}.len() > 0` first)"),
        "sorted" => format!("`{r}.sort()` sorts in place (`{r}` must be a `var`); `{r}.sort_by(x => key)` sorts by a key"),
        "rev" | "toreversed" => format!("`{r}.reversed()` gives a reversed copy; `{r}.reverse()` reverses in place"),
        "sortby" | "sortbykey" | "sortedby" => format!("`{r}.sort_by(x => key)` sorts in place by a key (`{r}` must be a `var`)"),
        "reduce" | "inject" | "aggregate" => {
            format!("`{r}.fold(start, (acc, x) => ...)` combines the elements, e.g. `{r}.fold(0, (acc, x) => acc + x)`")
        }
        "findindex" | "indexwhere" => format!("`{r}.find_index(x => test)` gives the position of the first match, or -1"),
        "some" | "anymatch" | "exists" => format!("`{r}.any(x => test)` is true if one element passes"),
        "every" | "allmatch" | "forall" => format!("`{r}.all(x => test)` is true if every element passes"),
        "select" | "where" => format!("`{r}.filter(x => test)` keeps the elements that pass"),
        "collect" | "transform" => format!("`{r}.map(x => ...)` gives a new array of the results"),
        "foreach" | "each" => format!("loop over the elements: `for x in {r} {{ ... }}` (`for i, x in {r}` also gives the position)"),
        "enumerate" | "withindex" | "indexed" => format!("`for i, x in {r} {{ ... }}` gives each position and element"),
        "clear" => format!("give it an empty array: `{r} = []` (`{r}` must be a `var`)"),
        "sublist" | "subarray" | "take" => {
            format!("`{r}.slice(a, b)` gives the elements from position `a` up to, but not including, `b`")
        }
        "concat" | "extend" | "addall" => format!("join arrays with `+`: `{r} + other`, or append in place with `{r} += other`"),
        "flatten" | "flat" => format!("join the inner arrays: `{r}.fold(empty, (acc, x) => acc + x)` or a loop with `+=`"),
        "zip" => format!("loop over the positions: `for i, x in {r} {{ ... other[i] ... }}`"),
        "average" | "mean" | "avg" => format!("divide the sum by the length: `float({r}.sum()) / float({r}.len())`"),
        _ => return None,
    })
}

fn str_method(w: &str, r: &str) -> Option<String> {
    Some(match w {
        "length" | "size" => format!("the length is `{r}.len()`"),
        "isempty" | "empty" => format!("compare with an empty string: `{r} == \"\"`"),
        "push" | "append" | "add" => format!("a string is joined with `+`: `{r} += t` (`{r}` must be a `var`; for a character write `{r} += str(c)`)"),
        "touppercase" | "uppercase" | "toupper" | "upcase" => format!("write `{r}.upper()` (it changes ASCII letters only)"),
        "tolowercase" | "lowercase" | "tolower" | "downcase" => format!("write `{r}.lower()` (it changes ASCII letters only)"),
        "strip" | "trimstart" | "trimend" | "trimleft" | "trimright" | "lstrip" | "rstrip" => {
            format!("`{r}.trim()` removes spaces, tabs and line breaks at both ends")
        }
        "startswith" => format!("write `{r}.starts_with(t)`"),
        "endswith" => format!("write `{r}.ends_with(t)`"),
        "indexof" | "find" | "search" => format!("`{r}.index_of(t)` gives the character position of the first match, or -1"),
        "includes" | "has" | "contain" => format!("write `{r}.contains(t)`"),
        "substring" | "substr" | "sub" => {
            format!("`{r}.slice(a, b)` gives the characters from position `a` up to, but not including, `b`")
        }
        "charat" | "at" | "get" => format!("`{r}[i]` is the character at position `i`, a `char`"),
        "charcodeat" | "codepointat" | "ord" => {
            format!("the code of the character at position `i` is `{r}[i].code()`; all the codes: `{r}.codes()`")
        }
        "replaceall" => format!("`{r}.replace(old, new)` already replaces every match"),
        "splitlines" | "lines" => format!("split at the line breaks: `{r}.split(\"\\n\")`"),
        "padstart" | "padend" | "ljust" | "rjust" | "center" | "zfill" => {
            format!("pad with `repeat`, e.g. `\" \".repeat(width - {r}.len()) + {r}` (the count must not be negative)")
        }
        "reverse" | "rev" => format!("`{r}.reversed()` gives the characters in reverse order"),
        "map" | "filter" => format!("strings have no `.{w}()`: use a comprehension over the characters, e.g. `[c.upper() for c in {r} if c != ' ']`, or `{r}.chars().{w}(c => ...)`"),
        "isdigit" | "isnumeric" | "isdecimal" | "isalpha" | "isalnum" | "isupper" | "islower" | "isspace" => {
            format!("these are `char` methods (`c.is_digit()`, `c.is_letter()`, ...): test each character, `for c in {r} {{ ... }}`")
        }
        "toint" | "parseint" | "parse" | "atoi" | "tonumber" | "parsefloat" | "tofloat" => {
            format!("parse text with `int({r})` or `float({r})` (a runtime error if it is not a number)")
        }
        "tochararray" | "tochars" | "tolist" => format!("`{r}.chars()` gives the characters as an array"),
        "bytes" | "encode" | "getbytes" => format!("strings are text, not bytes: `{r}.codes()` gives the character codes"),
        "concat" => format!("join strings with `+`: `{r} + other`"),
        "join" => "`join` belongs to an array of strings: `parts.join(\", \")`".to_string(),
        _ => return None,
    })
}

fn char_method(w: &str, r: &str) -> Option<String> {
    Some(match w {
        "isdigit" | "isnumeric" | "isdecimal" | "isasciidigit" => format!("write `{r}.is_digit()`"),
        "isalpha" | "isalphabetic" | "isletter" | "isasciialphabetic" => format!("write `{r}.is_letter()` (ASCII letters only)"),
        "isalnum" | "isalphanumeric" => format!("write `{r}.is_letter() || {r}.is_digit()`"),
        "isupper" | "isuppercase" => format!("write `{r}.is_upper()`"),
        "islower" | "islowercase" => format!("write `{r}.is_lower()`"),
        "isspace" | "iswhitespace" | "isblank" => format!("write `{r}.is_space()`"),
        "toupper" | "touppercase" | "uppercase" | "upcase" => format!("write `{r}.upper()`"),
        "tolower" | "tolowercase" | "lowercase" | "downcase" => format!("write `{r}.lower()`"),
        "ord" | "tocode" | "codepoint" | "charcode" | "ascii" | "asint" | "toint" | "tointeger" | "value" => {
            format!("the code of a character is `{r}.code()`")
        }
        _ => return None,
    })
}

/// The Nyra method that a method name of another language stands for, when it is the same
/// operation under another name (`xs.length()` is `xs.len()`, `s.toUpperCase()` is `s.upper()`).
/// Only exact equivalents are listed: `find` on an array takes a predicate in JavaScript, so it
/// is not `index_of`, and `substr(start, count)` is not `slice(a, b)`. The caller also checks that
/// the number of arguments fits.
pub fn method_rename(t: Type, name: &str) -> Option<&'static str> {
    let w: String = name.chars().filter(|c| *c != '_').map(|c| c.to_ascii_lowercase()).collect();
    Some(match (t, w.as_str()) {
        (Type::Array(_) | Type::Str, "length" | "size") => "len",
        (Type::Array(_) | Type::Str, "includes" | "has" | "contain") => "contains",
        (Type::Array(_) | Type::Str, "indexof") => "index_of",
        (Type::Array(_) | Type::Str, "rev" | "toreversed") => "reversed",
        (Type::Array(_) | Type::Str, "findindex") => "find_index",
        (Type::Array(_) | Type::Str, "every") => "all",
        (Type::Array(_) | Type::Str, "some") => "any",
        (Type::Array(_), "sortby" | "sortbykey") => "sort_by",
        (Type::Array(_), "append" | "add" | "pushback" | "addlast") => "push",
        (Type::Array(_), "popback" | "poplast" | "removelast") => "pop",
        (Type::Array(_), "removeat") => "remove",
        (Type::Str, "find") => "index_of",
        (Type::Str, "strip") => "trim",
        (Type::Str, "startswith") => "starts_with",
        (Type::Str, "endswith") => "ends_with",
        (Type::Str, "substring") => "slice",
        (Type::Str, "replaceall") => "replace",
        (Type::Str, "tochararray" | "tochars" | "tolist") => "chars",
        (Type::Str | Type::Char, "touppercase" | "uppercase" | "toupper" | "upcase") => "upper",
        (Type::Str | Type::Char, "tolowercase" | "lowercase" | "tolower" | "downcase") => "lower",
        (Type::Char, "isdigit" | "isnumeric" | "isdecimal" | "isasciidigit") => "is_digit",
        (Type::Char, "isalpha" | "isalphabetic" | "isletter" | "isasciialphabetic") => "is_letter",
        (Type::Char, "isupper" | "isuppercase") => "is_upper",
        (Type::Char, "islower" | "islowercase") => "is_lower",
        (Type::Char, "isspace" | "iswhitespace") => "is_space",
        (Type::Char, "ord" | "tocode" | "codepoint" | "charcode") => "code",
        _ => return None,
    })
}

/// True for names that other languages use for a type (`string`, `i32`, `double`, `void`, ...).
pub fn is_type_word(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "int"
            | "integer"
            | "long"
            | "short"
            | "number"
            | "byte"
            | "uint"
            | "usize"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "float"
            | "double"
            | "real"
            | "f32"
            | "f64"
            | "bool"
            | "boolean"
            | "str"
            | "string"
            | "char"
            | "void"
    )
}

/// The Nyra type that a type word of another language stands for (`string` is `str`, `i32` is `int`).
pub fn nyra_type(w: &str) -> Option<&'static str> {
    Some(match w.to_ascii_lowercase().as_str() {
        "string" | "text" | "cstring" | "varchar" | "str" => "str",
        "int" | "integer" | "long" | "short" | "number" | "byte" | "uint" | "usize" | "isize" | "size_t" | "i8" | "i16" | "i32"
        | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128" | "int32_t" | "int64_t" => "int",
        "float" | "double" | "real" | "decimal" | "single" | "f32" | "f64" | "float32" | "float64" => "float",
        "bool" | "boolean" => "bool",
        "char" | "character" | "rune" => "char",
        _ => return None,
    })
}

/// The Nyra type to write for a type name of another language, when there is exactly one:
/// `string` is `str` and `i32` is `int`, but TypeScript's `number` may be `int` or `float`.
pub fn type_fix(w: &str) -> Option<&'static str> {
    if w.eq_ignore_ascii_case("number") {
        return None;
    }
    nyra_type(w)
}

/// What to do when a value might be missing (`int?`, `Option<int>`): Nyra has no optional values.
pub const OPTION_HINT: &str = "an optional type is written with a `?` after the type, `int?`; a missing value is `none`, `x ?? d` gives a default and `if let v = x { ... }` takes the value out";

/// What to do about a type name that does not exist.
pub fn type_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if let Some(t) = nyra_type(name) {
        return format!("write `{t}` (the built-in type names are lowercase: `int`, `float`, `bool`, `str`, `char`)");
    }
    match lower.as_str() {
        "void" | "unit" | "none" | "nothing" => {
            "a function that returns nothing has no return type: leave out the `->` part".to_string()
        }
        "vec" | "list" | "array" | "vector" | "slice" | "arraylist" => {
            "an array type is written `[T]`, e.g. `[int]` or `[str]`".to_string()
        }
        "map" | "dict" | "hashmap" | "dictionary" | "object" | "record" => {
            "a map type is written `[K: V]`, e.g. `[str: int]`; a value is `[\"a\": 1]`, an empty one `[:]`".to_string()
        }
        "set" | "hashset" => "Nyra has no sets: use a map `[str: bool]` and `m.has(k)`, or an array and `xs.contains(x)`".to_string(),
        "tuple" | "pair" => "a tuple type is written with parentheses, e.g. `(int, str)`; a value `(1, \"a\")`".to_string(),
        "option" | "optional" | "maybe" | "nullable" => {
            "an optional type is written with a question mark after the type, e.g. `int?`".to_string()
        }
        "any" | "auto" | "var" | "let" | "dynamic" => {
            "write the type out (only local variables are inferred: leave the annotation off)".to_string()
        }
        _ => match suggest(name, ["int", "float", "bool", "str", "char"]) {
            Some(s) => format!("{s} The types are `int`, `float`, `bool`, `str`, `char`, arrays `[T]` and structs"),
            None => "the types are `int`, `float`, `bool`, `str`, `char`, arrays `[T]` and structs".to_string(),
        },
    }
}

/// The closing quote that matches an opening quote of another language, if `c` is one.
pub fn quote_close(c: char) -> Option<char> {
    match c {
        '`' => Some('`'),
        '\u{2018}' => Some('\u{2019}'),
        '\u{201C}' => Some('\u{201D}'),
        _ => None,
    }
}

/// How to write the text of a typographic or backtick literal in Nyra.
pub fn quoted_text(open: char, text: &str) -> String {
    if text.contains('"') || text.contains('\\') {
        return "Nyra text uses straight double quotes: \"like this\"".to_string();
    }
    let kind = match open {
        '`' => "Nyra has no backtick strings",
        _ => "typographic quotes are not quotes",
    };
    if open == '`' && text.contains("${") {
        return format!("{kind}: write double quotes and `{{x}}` instead of `${{x}}`, e.g. \"total: {{x}}\"");
    }
    format!("{kind}: write \"{text}\" with straight double quotes")
}

/// The text of a quoted literal of another language as a Nyra string, when that is certain:
/// `` `total: ${x}` `` is `"total: {x}"` and `“hi”` is `"hi"`. Text with braces (which would turn
/// into interpolation), quotes or backslashes, and one letter in single typographic quotes (a
/// `char`, or a string?) get no fix.
pub fn quoted_fix(open: char, text: &str) -> Option<String> {
    if text.contains(['"', '\\']) {
        return None;
    }
    if open == '`' && text.contains("${") {
        let mut out = String::new();
        let mut rest = text;
        while let Some(i) = rest.find("${") {
            let (head, tail) = rest.split_at(i);
            if head.contains(['{', '}']) {
                return None;
            }
            out += head;
            let close = tail.find('}')?;
            let inner = &tail[2..close];
            if inner.trim().is_empty() || inner.contains('{') {
                return None;
            }
            out += &format!("{{{inner}}}");
            rest = &tail[close + 1..];
        }
        if rest.contains(['{', '}']) {
            return None;
        }
        return Some(format!("\"{out}{rest}\""));
    }
    if text.contains(['{', '}']) || (open == '\u{2018}' && text.chars().count() == 1) {
        return None;
    }
    Some(format!("\"{text}\""))
}

/// What replaces a character that Nyra does not have, when it has exactly one meaning: `×` is `*`,
/// `≤` is `<=`, and an invisible character is removed. (`&` may be a bit operation, so it has none.)
pub fn char_fix(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{00D7}' => "*",
        '\u{00F7}' => "/",
        '\u{2212}' | '\u{2013}' | '\u{2014}' => "-",
        '\u{2264}' => "<=",
        '\u{2265}' => ">=",
        '\u{2260}' => "!=",
        '\u{2192}' => "->",
        '\u{2026}' => "..",
        '\u{2227}' => "&&",
        '\u{2228}' => "||",
        '\u{00AC}' => "!",
        c if invisible_char(c).is_some() => "",
        _ => return None,
    })
}

/// A name for characters that cannot be seen, so the message can say what they are.
pub fn invisible_char(c: char) -> Option<&'static str> {
    match c {
        '\u{FEFF}' => Some("byte order mark"),
        '\u{200B}' => Some("zero width space"),
        '\u{200C}' => Some("zero width non-joiner"),
        '\u{200D}' => Some("zero width joiner"),
        '\u{2060}' => Some("word joiner"),
        '\u{00AD}' => Some("soft hyphen"),
        c if c.is_control() => Some("control character"),
        _ => None,
    }
}

/// Hint for a character the lexer does not know.
pub fn bad_char(c: char) -> String {
    match c {
        '#' => "comments start with `//`, not `#`".into(),
        '&' => "write `&&` for logical and (there are no bit operations)".into(),
        '|' => "write `||` for logical or (there are no bit operations)".into(),
        '^' | '~' => "Nyra has no bit operations or power operator: multiply (`x * x`) or use a loop".into(),
        '@' => "`@` has no meaning in Nyra (there are no attributes or decorators)".into(),
        '$' => "to put a value in a string write `{x}` inside the quotes: \"cost: {x}\"".into(),
        '\\' => "a backslash only appears inside a string or a character, as in `\\n`".into(),
        '\u{00D7}' => "write `*` for multiplication".into(),
        '\u{00F7}' => "write `/` for division".into(),
        '\u{2212}' | '\u{2013}' | '\u{2014}' => "write a plain `-` (ASCII hyphen) for minus".into(),
        '\u{2264}' => "write `<=`".into(),
        '\u{2265}' => "write `>=`".into(),
        '\u{2260}' => "write `!=`".into(),
        '\u{2192}' => "write `->` for a return type".into(),
        '\u{2026}' => "write `..` for a range: `0..10`".into(),
        '\u{2227}' => "write `&&` for logical and".into(),
        '\u{2228}' => "write `||` for logical or".into(),
        '\u{00AC}' => "write `!` for logical not".into(),
        '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}' | '\u{201E}' => {
            "typographic quotes are not quotes: use straight double quotes (\") around text".into()
        }
        '\u{FEFF}' => "the file starts with a byte order mark: save it as UTF-8 without BOM".into(),
        c if invisible_char(c).is_some() => "remove it: it is an invisible character, often pasted in by accident".into(),
        _ => "this character is not part of Nyra: remove it".into(),
    }
}

/// Warning E0260 for `${code}` in a string, with `at` the position of the `$`: Nyra keeps the `$`
/// and inserts the value, which is rarely what a template engine's `${x}` was meant to say.
pub fn dollar_brace(code: &str, at: crate::ast::Span) {
    let code = code.trim();
    crate::diag::warn(
        crate::diag::Diag::new("E0260", format!("`${{{code}}}` in a string prints a `$` and then the value of `{code}`"), at).hint(format!(
            "`{{{code}}}` alone inserts the value, so drop the `$` if you did not mean to print one; to print a dollar sign before a value write `\"$\" + str(...)`, to print `${{{code}}}` as text double the braces: `${{{{{code}}}}}`"
        )),
    );
}
