fn main() {
    let mut term = String::from("2333111");
    for step in 1..=40 {
        let bytes = term.as_bytes();
        let mut next = String::with_capacity(bytes.len() * 2);
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            let mut j = i;
            while j < bytes.len() && bytes[j] == c {
                j += 1;
            }
            next.push_str(&(j - i).to_string());
            next.push(c as char);
            i = j;
        }
        term = next;
        if step % 5 == 0 {
            println!("{} {}", step, term.len());
        }
    }
    let ones = term.chars().filter(|&c| c == '1').count();
    let twos = term.chars().filter(|&c| c == '2').count();
    let threes = term.chars().filter(|&c| c == '3').count();
    println!("{} {} {}", ones, twos, threes);
}
