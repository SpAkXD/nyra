const W: i32 = 8;
const H: i32 = 6;

fn main() {
    let foods: [(i32, i32); 6] = [(4, 2), (2, 2), (6, 4), (0, 5), (7, 0), (3, 3)];
    let moves = "RRDLURLDDRDLLLLLLURDRR";
    // snake[0] is the head, last is the tail
    let mut snake: Vec<(i32, i32)> = vec![(2, 2), (1, 2), (0, 2)];
    let mut dir = (1, 0);
    let mut food_idx: usize = 0;
    let mut food: Option<(i32, i32)> = Some(foods[0]);
    let mut score = 0;
    let mut ended: Option<usize> = None;
    for (k, letter) in moves.chars().enumerate() {
        let d = match letter {
            'U' => (0, -1),
            'D' => (0, 1),
            'L' => (-1, 0),
            _ => (1, 0),
        };
        if !(d.0 == -dir.0 && d.1 == -dir.1) {
            dir = d;
        }
        let head = snake[0];
        let nh = (head.0 + dir.0, head.1 + dir.1);
        let grows = food == Some(nh);
        let tail = *snake.last().unwrap();
        let outside = nh.0 < 0 || nh.0 >= W || nh.1 < 0 || nh.1 >= H;
        let hits_body = snake[..snake.len() - 1].contains(&nh);
        let hits_tail = nh == tail && grows;
        if outside || hits_body || hits_tail {
            ended = Some(k + 1);
            break;
        }
        if grows {
            snake.insert(0, nh);
            score += 1;
            food = None;
            let mut i = food_idx + 1;
            while i < foods.len() {
                if !snake.contains(&foods[i]) {
                    food = Some(foods[i]);
                    break;
                }
                i += 1;
            }
            food_idx = i;
        } else {
            snake.insert(0, nh);
            snake.pop();
        }
    }
    match ended {
        Some(k) => println!("game over at move {}", k),
        None => println!("all moves done"),
    }
    println!("score {}", score);
    println!("length {}", snake.len());
    for y in 0..H {
        let row: String = (0..W)
            .map(|x| {
                if snake[0] == (x, y) {
                    'H'
                } else if snake.contains(&(x, y)) {
                    'o'
                } else if food == Some((x, y)) {
                    '*'
                } else {
                    '.'
                }
            })
            .collect();
        println!("{}", row);
    }
}
