import sys; sys.path.insert(0,'.')
import data, rewrites as R, rewrites2 as R2
M=dict(R.SYNTAX); M.update(R2.IDIOM)
L1=["neg_index","in_op","ternary","interp","pad_spec","bare_lambda","comprehension"]
H=["unwrap_main"]+L1+["implicit_ret","drop_local_ann","drop_ret_types","drop_let_var","drop_param_types","positional_struct","indent_blocks"]
def hform(src, steps=H):
    src=data.strip_nyra_comments(src)
    for s in steps: src=M[s](src)
    return src
if __name__=="__main__":
    c=data.reference() if sys.argv[1]=='ref' else data.model_pairs(sys.argv[1])
    for t in sys.argv[2:]:
        print('=== NYRA-H',t); print(hform(c[t]['nyra']))
        print('=== PY',t); print(data.strip_py_comments(c[t]['python']))
