fn sum_to(n: i64) -> i64 {
    let mut index: i64 = 0_i64;
    let mut total: i64 = 0_i64;

    while index <= n {
        total = total + index;
        index = index + 1_i64;
    }

    total
}

fn main() {
    println!("{}", sum_to(10_i64));
}
