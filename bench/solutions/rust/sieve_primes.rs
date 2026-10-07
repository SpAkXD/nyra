fn main() {
    let limit = 60;
    let mut is_prime = vec![true; limit];
    is_prime[0] = false;
    is_prime[1] = false;
    for i in 2..limit {
        if is_prime[i] {
            let mut multiple = i * i;
            while multiple < limit {
                is_prime[multiple] = false;
                multiple += i;
            }
        }
    }
    for (i, &flag) in is_prime.iter().enumerate() {
        if flag {
            println!("{}", i);
        }
    }
}
