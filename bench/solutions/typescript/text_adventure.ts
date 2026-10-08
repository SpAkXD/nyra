const opp: Record<string, string> = { north: "south", south: "north", east: "west", west: "east", down: "up", up: "down" };
const exits = new Map<string, Map<string, string>>();
const locked = new Set<string>();
function key(a: string, b: string): string { return a < b ? a + "|" + b : b + "|" + a; }
function link(a: string, d: string, b: string): void {
  if (!exits.has(a)) exits.set(a, new Map());
  if (!exits.has(b)) exits.set(b, new Map());
  exits.get(a)!.set(d, b);
  exits.get(b)!.set(opp[d], a);
}
link("hall", "north", "library");
link("hall", "east", "kitchen");
link("kitchen", "north", "pantry");
link("kitchen", "down", "cellar");
link("library", "east", "study");
link("study", "south", "pantry");
locked.add(key("library", "study"));
const roomItems = new Map<string, string[]>();
for (const r of exits.keys()) roomItems.set(r, []);
roomItems.get("hall")!.push("lamp");
roomItems.get("pantry")!.push("key", "rope");
roomItems.get("study")!.push("book");
roomItems.get("cellar")!.push("coin");
let room = "hall";
const carried: string[] = [];
const visited = new Set<string>([room]);
const cmds = "look; go north; unlock east; go east; go south; take lamp; go west; go east; go down; take coin; take coin; go up; go north; take key; drop coin; take key; inventory; go west; look; go north; unlock west; unlock west; go west; unlock east; drop lamp; go south; go east; go down; look; unlock up; take rope; go north; take coin; go south; go west; drop coin; inventory";
const list = (xs: string[]) => [...xs].sort().join(", ");
for (const c of cmds.split("; ")) {
  const [op, arg] = c.split(" ");
  const ex = exits.get(room)!;
  const here = roomItems.get(room)!;
  if (op === "go") {
    const to = ex.get(arg);
    if (to === undefined) console.log(`no exit ${arg}`);
    else if (locked.has(key(room, to))) console.log("the door is locked");
    else if (to === "cellar" && !carried.includes("lamp")) console.log("too dark to enter");
    else { room = to; visited.add(room); console.log(`you are in ${room}`); }
  } else if (op === "take") {
    const i = here.indexOf(arg);
    if (i < 0) console.log(`no ${arg} here`);
    else if (carried.length >= 2) console.log("hands full");
    else { here.splice(i, 1); carried.push(arg); console.log(`taken ${arg}`); }
  } else if (op === "drop") {
    const i = carried.indexOf(arg);
    if (i < 0) console.log(`you have no ${arg}`);
    else { carried.splice(i, 1); here.push(arg); console.log(`dropped ${arg}`); }
  } else if (op === "unlock") {
    const to = ex.get(arg);
    if (to === undefined) console.log(`no exit ${arg}`);
    else if (!locked.has(key(room, to))) console.log("nothing to unlock");
    else if (!carried.includes("key")) console.log("you need the key");
    else { locked.delete(key(room, to)); console.log("unlocked"); }
  } else if (op === "look") {
    const items = here.length ? list(here) : "none";
    console.log(`${room} | items: ${items} | exits: ${list([...ex.keys()])}`);
  } else if (op === "inventory") {
    console.log(`carrying: ${carried.length ? list(carried) : "nothing"}`);
  }
}
let score = 0;
for (const r of visited) if (r !== "hall") score += 10;
if (roomItems.get("hall")!.includes("coin")) score += 25;
console.log(`score ${score}`);
