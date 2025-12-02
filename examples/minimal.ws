// Minimal Wisp example for testing compiler phases

struct Point {
    x: i32,
    y: i32,
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    let x = 5
    let mut y = 10
    
    y = y + 1
    
    let sum = add(x, y)
    
    let p = Point { x: 1, y: 2 }
    
    if sum > 10 {
        let z = sum * 2
    } else {
        let z = 0
    }
    
    let mut i = 0
    while i < 5 {
        i = i + 1
    }
}

