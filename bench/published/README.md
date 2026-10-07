# Published benchmark results

Summaries of real benchmark runs, written by `python bench/publish.py` (see `bench/README.md`). Each summary is
a pair of files with the same name: `<name>.md` to read and `<name>.json` for tools (a chart generator, say).

They hold the headline numbers per model and language, a per-category breakdown and a few notable failures.
They do **not** hold prompts, replies or programs: the raw result files stay in `bench/results/`, which is
git-ignored. Each summary lists its sources with the checksum of every raw file.

Nothing here was produced by a mock run: `publish.py` refuses those.
