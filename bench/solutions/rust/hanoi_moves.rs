// Move m (counting from 1) goes from peg (m & (m - 1)) % 3 to peg ((m | (m - 1)) + 1) % 3. Starting on peg 0, an even
// number of disks ends on peg 1, so peg 1 is C and peg 2 is the spare B.
fn main() {
    let disks = 4;
    let names = ["A", "C", "B"];
    for m in 1u32..(1 << disks) {
        let from = (m & (m - 1)) % 3;
        let to = ((m | (m - 1)) + 1) % 3;
        println!("{} -> {}", names[from as usize], names[to as usize]);
    }
}
