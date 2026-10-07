fn money(cents: u32) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn line(label: &str, cents: u32) {
    println!("{:<8}{:>8}", label, money(cents));
}

fn main() {
    let items = [("apple", 125, 3), ("bread", 399, 1), ("milk", 89, 12), ("cheese", 1450, 2)];
    let mut subtotal = 0;
    for (name, price, quantity) in items {
        let total = price * quantity;
        subtotal += total;
        line(name, total);
    }
    let tax = (subtotal * 825 + 5000) / 10000;
    line("subtotal", subtotal);
    line("tax", tax);
    line("total", subtotal + tax);
}
