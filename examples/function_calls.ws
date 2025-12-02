// Test multiple function calls

fn double(x: i32) -> i32 {
    x * 2
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn compute(x: i32, y: i32) -> i32 {
    let a = double(x)
    let b = double(y)
    add(a, b)
}

fn main() -> i32 {
    compute(3, 4)  // double(3) + double(4) = 6 + 8 = 14
}

