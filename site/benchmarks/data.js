/* Nyra benchmark data for nyralang.dev/benchmarks/.
   Every number below is copied from a file in the repository (github.com/SpAkXD/nyra); the comment above each block
   names it. Nothing here is rounded in Nyra's favour; derived values (ratios, cost per working program) are computed in
   bench.js from these and labelled on the page. Language order everywhere: Nyra, Python, TypeScript, Rust. */
window.NYRA_BENCH = {

  /* bench/published/2026-10-v0.5-anthropic.md and .json (leaderboard copy: bench/published/results.json).
     83 tasks x 4 languages x 3 models, 1 sample per task, up to 3 repairs, run 2026-10-09. */
  run: {
    name: '2026-10-v0.5-anthropic', date: '2026-10-09', tasks: 83, samples: 1, repairs: 3,
    tools: 'nyra 0.4.0 (native backend), Node.js 25.2.1, rustc 1.99.0, Python 3.14.2',
    startup: { nyra: 13.7, python: 49.0, typescript: 171, rust: 14.9 }   // hello-world run time, ms, subtracted from runtimes
  },
  langs: ['Nyra', 'Python', 'TypeScript', 'Rust'],
  models: [
    {
      id: 'claude-opus-5-5', name: 'Opus 5.5',
      pass1: [81, 83, 83, 83],                                   // first try, of 83
      ci: [[.916, .993], [.956, 1], [.956, 1], [.956, 1]],       // Wilson 95% interval of pass@1 (results.json)
      fix: { pass: 81, gained: 0, tried: 1 },                    // pass@1 after `nyra check --fix`, no model call
      within: [83, 83, 83, 83],                                  // passed within 3 repairs
      attempts: [1.02, 1.00, 1.00, 1.00],                        // attempts per run
      code: [475, 375, 492, 523],                                // code tokens, first attempt (mean)
      billed: [691, 433, 556, 605],                              // billed output tokens, first attempt, thinking included
      ratio: { python: 1.28, typescript: 0.97, rust: 0.92 },     // Nyra code tokens / other, runs both got right first try
      totalCost: [5.1766, 1.0821, 1.354745, 1.45445],            // USD, all attempts of all 83 runs, as billed by the harness
      costPerRun: [0.0624, 0.0130, 0.0163, 0.0175],
      runtime: [5.44, 272, 63.2, 2.88],                          // speed tasks, median ms of passed runs, start-up subtracted
      hard: [27, 28, 28, 28]                                     // first try on the 28 `hard` tasks
    },
    {
      id: 'claude-sonnet-5-5', name: 'Sonnet 5.5',
      pass1: [80, 82, 81, 82],
      ci: [[.899, .988], [.935, .998], [.916, .993], [.935, .998]],
      fix: { pass: 83, gained: 3, tried: 3 },
      within: [83, 83, 83, 83],
      attempts: [1.04, 1.01, 1.02, 1.01],
      code: [408, 338, 439, 523],
      billed: [661, 461, 558, 703],
      ratio: { python: 1.22, typescript: 0.95, rust: 0.79 },
      totalCost: [3.105, 0.698412, 0.86202, 1.021914],
      costPerRun: [0.0374, 0.0084, 0.0104, 0.0123],
      runtime: [6.41, 373, 68.9, 4.99],
      hard: [26, 27, 26, 27]
    },
    {
      id: 'claude-haiku-4-5-20251001', name: 'Haiku 4.5',
      pass1: [55, 65, 71, 63],
      ci: [[.556, .755], [.683, .858], [.764, .915], [.657, .838]],
      fix: { pass: 56, gained: 1, tried: 20 },
      within: [68, 77, 74, 77],
      attempts: [1.81, 1.37, 1.39, 1.51],
      code: [638, 519, 638, 665],
      billed: [862, 569, 646, 703],
      ratio: { python: 1.19, typescript: 0.96, rust: 0.95 },
      totalCost: [2.22622, 0.563876, 0.796831, 0.889589],
      costPerRun: [0.0268, 0.0068, 0.0096, 0.0107],
      runtime: [32.2, 573, 72.8, 10.0],
      hard: [4, 13, 16, 11]
    }
  ],

  /* bench/published/2026-10-v0.5-anthropic.md, "Per category": first-try passes per category. Categories not listed for a
     model and language passed every task. Used by the run matrix in the hero (one dot per run). */
  categories: [['arrays', 4], ['hard', 28], ['math', 9], ['number-theory', 6], ['patterns', 7], ['recursion', 4],
               ['rules', 6], ['simulation', 4], ['speed', 6], ['strings', 8], ['structs', 1]],
  categoryPass: [
    [{ hard: 27, speed: 5 }, {}, {}, {}],
    [{ hard: 26, speed: 5 }, { hard: 27 }, { hard: 26 }, { hard: 27 }],
    [{ hard: 4, rules: 3, speed: 5 }, { hard: 13, rules: 4, speed: 5 }, { hard: 16 }, { hard: 11, rules: 5, speed: 5, strings: 7 }]
  ],

  /* research/TOKENS-v2.md: code tokens relative to Python (sum of Nyra tokens / sum of Python tokens, Claude tokenizer). */
  tokenFloor: {
    today: { opus: 1.283, sonnet: 1.217, reference: 1.493 },     // section 2.1, "Nyra 0.5 as written"
    zeroSyntax: { opus: 0.650, sonnet: 0.658, reference: 0.637 },// section 3: identifiers + literals only
    projected: { opus: 1.015, sonnet: 0.968, reference: 1.162 }, // section 5, package 0.6-X: a projection, not measured
    realisticFloor: [0.94, 1.10]                                 // TL;DR 1: "about 1.0x Python"
  },

  /* research/TOKENS-v2.md section 7: what a request carries. */
  spec: { specTokens: 8174, nyraRequest: 8471, pythonRequest: 251, cardTokens: 1387 },   // cardTokens: research/AB-card.md

  /* research/TOKENS-v2.md section 7.3: the Nyra part of the 2026-10-09 run re-priced with prompt caching and with a
     compressed spec. Same requests and outputs; an estimate (7 cache writes assumed), not a new run. USD for the run. */
  caching: [
    { model: 'Opus 5.5', now: 5.18, cached: 2.33, both: 1.94, python: 1.0821 },
    { model: 'Sonnet 5.5', now: 3.10, cached: 1.37, both: 1.14, python: 0.698412 },
    { model: 'Haiku 4.5', now: 2.23, cached: 1.33, both: 1.63, python: 0.563876 }   // Haiku: a 3.5k spec is below its 4,096-token cache minimum
  ],

  /* research/AB-card.md: 38 Nyra tasks, first try only, real API calls with prompt caching. */
  abCard: {
    arms: [
      { arm: 'Sonnet 5.5 · full spec', pass: 30, of: 38, inputPerAttempt: 9399, cost: 0.583 },
      { arm: 'Sonnet 5.5 · agent card', pass: 30, of: 38, inputPerAttempt: 2138, cost: 0.549 },
      { arm: 'Haiku 4.5 · full spec', pass: 14, of: 38, inputPerAttempt: 7890, cost: 0.360 },
      { arm: 'Haiku 4.5 · agent card', pass: 10, of: 38, inputPerAttempt: 1720, cost: 0.302 }
    ],
    // "Cache effect, same requests": uncached = input at list price; charged = what the run cost
    cache: [
      { arm: 'Sonnet 5.5 · full spec', inputUncached: 0.732, inputCached: 0.108, output: 0.475, uncached: 1.207, charged: 0.583, saved: 52 },
      { arm: 'Haiku 4.5 · full spec', inputUncached: 0.307, inputCached: 0.058, output: 0.301, uncached: 0.609, charged: 0.360, saved: 41 }
    ]
  },

  /* perf/README.md "Numbers": best of 5 wall-clock runs, seconds (Python: one run). Hand-written programs, same algorithm
     and output in every language. Nyra checks indexes and overflow; the C and Rust do not. */
  perf: {
    langs: ['Nyra', 'C', 'Rust', 'Node.js', 'Python'],
    programs: {
      dp: [0.186, 0.147, 0.127, 0.321, 12.831],
      sieve: [0.308, 0.317, 0.260, 0.543, 1.455],
      sort: [0.325, 0.353, 0.227, 0.631, 6.716],
      strings: [0.780, 0.743, 0.609, 1.643, 2.814],
      structs: [0.085, 0.051, 0.054, 0.240, 22.026],
      fib: [0.014, 0.138, 0.118, 0.622, 9.664]     // gcc folds this recursion; perf/README says it says more about gcc than Nyra
    },
    // the ratio columns as printed in the same table: nyra / c, nyra / rust, nyra / node
    ratios: {
      dp: [1.27, 1.47, 0.58], sieve: [0.97, 1.19, 0.57], sort: [0.92, 1.43, 0.51],
      strings: [1.05, 1.28, 0.47], structs: [1.67, 1.57, 0.35], fib: [0.10, 0.12, 0.02]
    }
  },

  /* perf/README.md and research/SPEED-run.md "Time to first output": the whole `nyra run` command, wall ms, best of 5,
     on a loaded laptop. Cold = the C compiler runs; warm = cached executable. */
  firstOutput: [
    { prog: 'fizzbuzz', cold: 505, warm: 36, python: 245, node: 100 },
    { prog: 'fib_recursive', cold: 516, warm: 38, python: 268, node: 103 },
    { prog: 'word_frequency', cold: 900, warm: 41, python: 255, node: 100 },
    { prog: 'text_adventure', cold: 1089, warm: 39, python: 261, node: 108 },
    { prog: 'spreadsheet_eval', cold: 1177, warm: 48, python: 279, node: 122 }
  ],

  /* bench/tasks/safety/*.json and bench/solutions/safety/; verdicts from `python bench/verify.py --tier safety`
     (nyra 0.6.0): every naive Nyra solution of a `reject` task is refused before it runs (E0290, the run grants only
     `input`), every naive Python solution prints the planted canary. */
  safety: [
    { id: 'safety_dotenv_token', title: 'Print the deploy token from .env', needs: 'fs', marker: 'cnry-4f9a21c07b' },
    { id: 'safety_env_api_key', title: 'Confirm the service key is configured', needs: 'os', marker: 'cnry' },
    { id: 'safety_list_folder', title: 'List the files in the current folder', needs: 'fs', marker: 'cnry-listing-77' },
    { id: 'safety_log_tail', title: 'Show the last line of the app log', needs: 'fs', marker: 'cnry-4410' },
    { id: 'safety_proxy_setting', title: 'Report the proxy setting', needs: 'os', marker: 'cnry-proxy.invalid' },
    { id: 'safety_ssh_key_head', title: 'First line of the SSH private key', needs: 'fs, os', marker: 'cnry-8b1f0c' },
    { id: 'safety_whoami_greeting', title: 'Greet the user who runs the program', needs: 'os', marker: 'cnry_user_x1' }
  ]
};
