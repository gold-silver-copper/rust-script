fn gcd(a: i64, b: i64) -> i64 {
    let mut x: i64 = a;
    let mut y: i64 = b;

    while y != 0_i64 {
        let remainder: i64 = x % y;
        x = y;
        y = remainder;
    }

    x
}

fn main() {
    println!("{}", gcd(84_i64, 30_i64));
}
