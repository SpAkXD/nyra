// array-of-structs updates (reference for structs.nyra)
#[derive(Clone, Copy)]
struct Particle {
    x: i64,
    y: i64,
    vx: i64,
    vy: i64,
}

fn main() {
    let mut seed: i64 = 12345;
    let mut ps: Vec<Particle> = Vec::new();
    for _ in 0..100_000 {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        let x = (seed / 65536) % 1000;
        seed = (seed * 1103515245 + 12345) % 2147483648;
        let y = (seed / 65536) % 1000;
        seed = (seed * 1103515245 + 12345) % 2147483648;
        ps.push(Particle { x, y, vx: (seed / 65536) % 7 - 3, vy: (seed / 1024) % 5 - 2 });
    }
    for _ in 0..200 {
        for p in ps.iter_mut() {
            p.x += p.vx;
            p.y += p.vy;
            if p.x < 0 || p.x >= 1000 {
                p.vx = -p.vx;
            }
            if p.y < 0 || p.y >= 1000 {
                p.vy = -p.vy;
            }
        }
    }
    let sx: i64 = ps.iter().map(|p| p.x).sum();
    let sy: i64 = ps.iter().map(|p| p.y).sum();
    println!("{sx} {sy}");
}
