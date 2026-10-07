const commands = [
  "open A 100", "open B 50", "open A 10", "deposit C 5", "withdraw B 70", "transfer A B 60", "transfer B A 112",
  "transfer B A 109", "transfer A A 5", "withdraw A 25", "close B", "transfer A B 1", "deposit A 7", "withdraw A 15",
  "close A", "open B 0", "close B",
];

const balances = new Map<string, number>();

function run(command: string): string {
  const [op, ...args] = command.split(" ");
  const x = args[0];
  switch (op) {
    case "open":
      if (balances.has(x)) return `${x} exists`;
      balances.set(x, Number(args[1]));
      return `opened ${x}`;
    case "deposit":
      if (!balances.has(x)) return `${x} unknown`;
      balances.set(x, balances.get(x)! + Number(args[1]));
      return `${x} balance ${balances.get(x)}`;
    case "withdraw": {
      if (!balances.has(x)) return `${x} unknown`;
      const after = balances.get(x)! - Number(args[1]);
      if (after < 0) return `${x} insufficient`;
      balances.set(x, after);
      return `${x} balance ${after}`;
    }
    case "transfer": {
      const y = args[1];
      const n = Number(args[2]);
      if (!balances.has(x) || !balances.has(y)) return "unknown account";
      if (balances.get(x)! < n + 1) return `${x} insufficient`;
      if (x === y) {
        balances.set(x, balances.get(x)! - 1);
      } else {
        balances.set(x, balances.get(x)! - n - 1);
        balances.set(y, balances.get(y)! + n);
      }
      return `${x} balance ${balances.get(x)} ${y} balance ${balances.get(y)}`;
    }
    case "close":
      if (!balances.has(x)) return `${x} unknown`;
      if (balances.get(x) !== 0) return `${x} not empty`;
      balances.delete(x);
      return `closed ${x}`;
  }
  throw new Error(`unknown command ${command}`);
}

for (const command of commands) {
  console.log(run(command));
}
