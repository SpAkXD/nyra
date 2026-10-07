struct Rect {
    width: u32,
    height: u32,
}

fn main() {
    let rects = [
        Rect { width: 3, height: 4 },
        Rect { width: 10, height: 2 },
        Rect { width: 7, height: 7 },
    ];
    for rect in &rects {
        let area = rect.width * rect.height;
        let perimeter = 2 * (rect.width + rect.height);
        println!("{} {}", area, perimeter);
    }
}
