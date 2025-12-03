// Test all working features

fn abs(x: i32) -> i32 {
    if x < 0 {
        0 - x
    } else {
        x
    }
}

fn max(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

fn min(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

fn sum_to(n: i32) -> i32 {
    let mut total = 0
    let mut i = 1
    while i <= n {
        total = total + i
        i = i + 1
    }
    total
}

fn is_even(n: i32) -> i32 {
    if n % 2 == 0 {
        1
    } else {
        0
    }
}

fn main() -> i32 {
    // Test arithmetic
    let a = 10 + 5      // 15
    let b = a * 2       // 30
    let c = b - 10      // 20
    let d = c / 4       // 5

    // Test comparisons
    let e = if d == 5 { 1 } else { 0 }  // 1

    // Test function calls
    let f = abs(0 - 7)  // 7
    let g = max(3, 8)   // 8
    let h = min(3, 8)   // 3

    // Test loop
    let i = sum_to(5)   // 15

    // Test modulo
    let j = is_even(4)  // 1

    // Combine results: d + e + f + g + h + i + j = 5 + 1 + 7 + 8 + 3 + 15 + 1 = 40
    d + e + f + g + h + i + j
}
