use std::collections::BTreeMap;

const TEXT: &str = "The rabbit's hole was deep; the rabbit didn't stop. 'Down, down, down!' said Alice -- and down-hill she \
went, past rabbit-holes, shelves, maps and jars. Was it 3 o'clock? It wasn't: the clock said half-past 4. ''Curious,'' \
thought Alice, ''curiouser'' -- and the rabbit's watch said nothing at all. DOWN went the jars, down went the maps; \
Alice didn't mind, didn't care, didn't stop. The rabbit's ears twitched; the rabbit whispered.";

const STOP: [&str; 18] = [
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "her", "was", "one", "our", "out", "his",
    "has",
];

fn main() {
    let lower = TEXT.to_lowercase();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for cand in lower.split(|c: char| !(c.is_ascii_lowercase() || c == '\'')) {
        let w = cand.trim_matches('\'');
        if w.len() < 3 || STOP.contains(&w) {
            continue;
        }
        *counts.entry(w.to_string()).or_insert(0) += 1;
    }
    let mut list: Vec<(&String, &usize)> = counts.iter().collect();
    list.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (w, c) in list.iter().take(8) {
        let c = c.to_string();
        println!("{}{}{}", w, ".".repeat(24 - w.len() - c.len()), c);
    }
    println!("distinct: {}", counts.len());
    let mut longest = "";
    for w in counts.keys() {
        if w.len() > longest.len() {
            longest = w;
        }
    }
    println!("longest: {}", longest);
}
