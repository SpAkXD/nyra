// Nyra as a tool for model APIs: the specs, and the handler that runs a call.

export const TOOL_NAME = "run_nyra";
export const TOOL_DESCRIPTION =
  "Run a program written in Nyra in a sandbox and get back its standard output, or its errors. " +
  "Nyra is a small statically typed language: `let` (immutable) and `var`, `fn name(a: int) -> int { ... }`, " +
  "`struct`, `enum`, `match`, `for x in xs`, `for i in 0..n`, arrays `[int]`, maps `[str: int]`, " +
  "string interpolation \"{expr}\". Print results with `print(...)`; `use json` and `print(json.str(value))` " +
  "prints a value as JSON. Read the `stdin` argument with `use input` and `input.line()` / `input.all()`. " +
  "No files, network or OS access. Every error comes back with a code (E0xxx), a line and a hint: fix the " +
  "code and call the tool again.";

export const PARAMETERS = {
  type: "object",
  properties: {
    code: { type: "string", description: "The complete Nyra program." },
    stdin: { type: "string", description: "Standard input for the program (optional)." },
  },
  required: ["code"],
  additionalProperties: false,
};

const schema = () => JSON.parse(JSON.stringify(PARAMETERS));

/** A tool for the Anthropic Messages API (`tools: [...]`). */
export function anthropicToolSpec(name = TOOL_NAME, description = TOOL_DESCRIPTION) {
  return { name, description, input_schema: schema() };
}

/** A function tool for OpenAI-style chat completions (`tools: [...]`). */
export function openaiToolSpec(name = TOOL_NAME, description = TOOL_DESCRIPTION) {
  return { type: "function", function: { name, description, parameters: schema() } };
}

/** Runs the input of a `run_nyra` call ({code, stdin}) in `sandbox` and returns the text for the result. */
export async function runTool(input, sandbox) {
  if (!sandbox) {
    const { Sandbox } = await import("./index.js");
    sandbox = new Sandbox();
  }
  const r = await sandbox.run(String(input.code ?? ""), String(input.stdin ?? ""));
  return r.feedback();
}
