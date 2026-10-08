fn energy(pos: &[[i64; 3]; 5], vel: &[[i64; 3]; 5]) -> i64 {
    let mut total = 0;
    for b in 0..5 {
        let p: i64 = pos[b].iter().map(|v| v.abs()).sum();
        let k: i64 = vel[b].iter().map(|v| v.abs()).sum();
        total += p * k;
    }
    total
}

fn main() {
    let mut pos: [[i64; 3]; 5] = [[-7, 17, -11], [9, 12, 5], [-9, 0, -4], [4, 6, 0], [1, -13, 8]];
    let mut vel = [[0i64; 3]; 5];
    for step in 1..=20000 {
        for a in 0..5 {
            for b in a + 1..5 {
                for axis in 0..3 {
                    if pos[a][axis] < pos[b][axis] {
                        vel[a][axis] += 1;
                        vel[b][axis] -= 1;
                    } else if pos[a][axis] > pos[b][axis] {
                        vel[a][axis] -= 1;
                        vel[b][axis] += 1;
                    }
                }
            }
        }
        for b in 0..5 {
            for axis in 0..3 {
                pos[b][axis] += vel[b][axis];
            }
        }
        if step % 4000 == 0 {
            println!("{}", energy(&pos, &vel));
        }
    }
    for b in 0..5 {
        println!("{} {} {} {} {} {}", pos[b][0], pos[b][1], pos[b][2], vel[b][0], vel[b][1], vel[b][2]);
    }
}
