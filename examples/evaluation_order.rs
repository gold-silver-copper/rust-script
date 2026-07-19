fn left() -> i64 {
    println!("{}", 1_i64);
    10_i64
}

fn right() -> i64 {
    println!("{}", 2_i64);
    20_i64
}

fn main() {
    println!("{}", left() + right());
}
