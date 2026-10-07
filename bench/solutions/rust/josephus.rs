fn survivor(n: usize, k: usize) -> usize {
    let mut people: Vec<usize> = (1..=n).collect();
    let mut index = 0;
    while people.len() > 1 {
        index = (index + k - 1) % people.len();
        people.remove(index);
    }
    people[0]
}

fn main() {
    for (n, k) in [(7, 3), (41, 3), (10, 2)] {
        println!("{}", survivor(n, k));
    }
}
