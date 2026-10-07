struct Rect {
    width: u32,
    height: u32,
}

fn area(rect: &Rect) -> u32 {
    rect.width * rect.height
}

fn perimeter(rect: &Rect) -> u32 {
    2 * (rect.width + rect.height)
}

fn main() {
    let rects = [
        Rect { width: 3, height: 4 },
        Rect { width: 10, height: 2 },
        Rect { width: 7, height: 7 },
    ];
    for rect in &rects {
        println!("{} {}", area(rect), perimeter(rect));
    }
}
