fn is_palindrome(word: &str) -> bool {
    let bytes = word.as_bytes();
    if bytes.is_empty() {
        return true;
    }
    let mut i = 0;
    let mut j = bytes.len() - 1;
    while i < j {
        if bytes[i] != bytes[j] {
            return false;
        }
        i += 1;
        j -= 1;
    }
    true
}

fn main() {
    for word in ["level", "hello", "racecar", "robot", "a", "abba"] {
        println!("{}", if is_palindrome(word) { "yes" } else { "no" });
    }
}
