// quicksort and the standard stable sort of 1,000,000 ints (reference for sort.nyra)
fn quicksort(xs: &mut [i64], lo: i64, hi: i64) {
    if lo >= hi {
        return;
    }
    let pivot = xs[((lo + hi) / 2) as usize];
    let (mut i, mut j) = (lo, hi);
    while i <= j {
        while xs[i as usize] < pivot {
            i += 1;
        }
        while xs[j as usize] > pivot {
            j -= 1;
        }
        if i <= j {
            xs.swap(i as usize, j as usize);
            i += 1;
            j -= 1;
        }
    }
    quicksort(xs, lo, j);
    quicksort(xs, i, hi);
}

fn main() {
    let mut seed: i64 = 7;
    let mut xs: Vec<i64> = Vec::new();
    for _ in 0..1_000_000 {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        xs.push((seed / 64) % 1000000);
    }
    let mut ys = xs.clone();
    let n = xs.len() as i64;
    quicksort(&mut xs, 0, n - 1);
    ys.sort();
    println!("{}", xs == ys);
    let mut check: i64 = 0;
    for x in &xs {
        check = (check * 31 + x) % 1000000007;
    }
    println!("{check}");
}
