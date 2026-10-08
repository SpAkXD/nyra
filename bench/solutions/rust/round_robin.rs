use std::collections::VecDeque;

fn main() {
    let jobs: [(&str, i64, i64); 6] = [("A", 0, 5), ("B", 1, 3), ("C", 3, 8), ("D", 11, 2), ("E", 30, 4), ("F", 30, 1)];
    let n = jobs.len();
    let mut remaining: Vec<i64> = jobs.iter().map(|j| j.2).collect();
    let mut joined = vec![false; n];
    let mut finish = vec![0i64; n];
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut t: i64 = 0;
    let mut prev: Option<usize> = None;
    let mut idle = false;
    let mut pending: Option<usize> = None;
    loop {
        for i in 0..n {
            if !joined[i] && jobs[i].1 <= t {
                joined[i] = true;
                queue.push_back(i);
            }
        }
        if let Some(p) = pending.take() {
            queue.push_back(p);
        }
        let job = match queue.pop_front() {
            Some(j) => j,
            None => {
                let next = (0..n).filter(|&i| !joined[i]).map(|i| jobs[i].1).min();
                match next {
                    None => break,
                    Some(at) => {
                        println!("{}-{} idle", t, at);
                        t = at;
                        idle = true;
                        continue;
                    }
                }
            }
        };
        if let Some(p) = prev {
            if p != job && !idle {
                println!("{}-{} switch", t, t + 1);
                t += 1;
            }
        }
        let run = remaining[job].min(3);
        println!("{}-{} {}", t, t + run, jobs[job].0);
        t += run;
        remaining[job] -= run;
        if remaining[job] > 0 {
            pending = Some(job);
        } else {
            finish[job] = t;
        }
        prev = Some(job);
        idle = false;
    }
    for i in 0..n {
        println!("{} finish {} wait {}", jobs[i].0, finish[i], finish[i] - jobs[i].1 - jobs[i].2);
    }
}
