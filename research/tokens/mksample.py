"""Writes the 24-program sample: original Opus Nyra (comments stripped), its Python twin, and the mechanical
'X start' version (script + library idioms + implicit ret + inferred local/return types + indentation) that is then
edited by hand to use tuples, destructuring, sorted-by-key, etc."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import data
import rewrites as R, rewrites2 as R2
M = dict(R.SYNTAX); M.update(R2.IDIOM)
SAMPLE = """palindromes digit_sums gcd_pairs struct_rectangles word_wrap happy_numbers binary_search roman_numerals
look_and_say receipt lcs_table matrix_mult word_frequency line_diff bank_ledger rle_codec seat_booking four_in_row
fraction_total table_format league_table fib_labelled config_parser sparse_ledger""".split()
L1 = ["neg_index", "in_op", "ternary", "interp", "pad_spec", "bare_lambda", "comprehension"]
START = ["unwrap_main"] + L1 + ["implicit_ret", "drop_local_ann", "drop_ret_types", "indent_blocks"]
c = data.model_pairs("opus")
here = os.path.dirname(os.path.abspath(__file__))
for t in SAMPLE:
    ny = data.strip_nyra_comments(c[t]["nyra"])
    open(f"{here}/sample/orig/{t}.nyra", "w", encoding="utf-8", newline="\n").write(ny + "\n")
    open(f"{here}/sample/py/{t}.py", "w", encoding="utf-8", newline="\n").write(data.strip_py_comments(c[t]["python"]) + "\n")
    s = ny
    for st in START:
        s = M[st](s)
    p = f"{here}/sample/X/{t}.nyra"
    if not os.path.exists(p):
        open(p, "w", encoding="utf-8", newline="\n").write(s + "\n")
print(len(SAMPLE), "programs")
