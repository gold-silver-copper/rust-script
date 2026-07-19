fn mark() -> bool {
    println!("{}", 99_i64);
    true
}

fn main() {
    println!("{}", false && mark());
    println!("{}", true || mark());
}
