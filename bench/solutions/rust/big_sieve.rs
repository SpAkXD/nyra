fn main() {
    let limit: usize = 2_000_000;
    let mut is_prime = vec![true; limit];
    is_prime[0] = false;
    is_prime[1] = false;
    let mut i = 2;
    while i * i < limit {
        if is_prime[i] {
            let mut j = i * i;
            while j < limit {
                is_prime[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    let mut count: u64 = 0;
    let mut total: u64 = 0;
    let mut largest = 0;
    let mut twins: u64 = 0;
    for n in 2..limit {
        if is_prime[n] {
            count += 1;
            total += n as u64;
            largest = n;
            if n + 2 < limit && is_prime[n + 2] {
                twins += 1;
            }
        }
    }
    println!("{}", count);
    println!("{}", total);
    println!("{}", largest);
    println!("{}", twins);
}
