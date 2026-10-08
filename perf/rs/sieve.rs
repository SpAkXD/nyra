// sieve of Eratosthenes to 10,000,000, ten rounds (reference for sieve.nyra)
fn count_primes(n: usize) -> i64 {
    let mut sieve = vec![true; n + 1];
    sieve[0] = false;
    sieve[1] = false;
    let mut i = 2;
    while i * i <= n {
        if sieve[i] {
            let mut j = i * i;
            while j <= n {
                sieve[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    sieve.iter().filter(|b| **b).count() as i64
}

fn main() {
    let mut total = 0;
    for round in 0..10 {
        total += count_primes(10_000_000 - round);
    }
    println!("{total}");
}
