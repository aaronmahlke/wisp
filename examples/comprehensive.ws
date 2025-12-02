// Comprehensive Wisp example - tests more language features

// === Structs ===
struct Point {
    x: i32,
    y: i32,
}

struct Rectangle {
    origin: Point,
    width: i32,
    height: i32,
}

// === Enums ===
enum Option {
    Some(value: i32),
    None,
}

enum Result {
    Ok(value: i32),
    Err(code: i32, message: String),
}

// === Traits ===
trait Display {
    fn fmt(&self) -> String
}

trait Clone {
    fn clone(&self) -> Self
}

// === Impl blocks ===
impl Point {
    fn new(x: i32, y: i32) -> Point {
        Point { x: x, y: y }
    }
    
    fn origin() -> Point {
        Point { x: 0, y: 0 }
    }
    
    fn distance(&self, other: &Point) -> i32 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        dx * dx + dy * dy
    }
}

impl Display for Point {
    fn fmt(&self) -> String {
        "Point"
    }
}

// === Functions with references ===
fn swap(a: &mut i32, b: &mut i32) {
    let tmp = *a;
    *a = *b;
    *b = tmp;
}

fn first(arr: &[i32]) -> &i32 {
    &arr[0]
}

// === Generics (future) ===
// fn identity<T>(x: T) -> T {
//     x
// }

// === Control flow ===
fn fibonacci(n: i32) -> i32 {
    if n < 2 {
        n
    } else {
        fibonacci(n - 1) + fibonacci(n - 2)
    }
}

fn factorial(n: i32) -> i32 {
    let mut result = 1
    let mut i = 1
    while i <= n {
        result = result * i
        i = i + 1
    }
    result
}

// === Pattern matching ===
fn unwrap_or(opt: Option, default: i32) -> i32 {
    match opt {
        Some(v) -> v,
        None -> default,
    }
}

// === Main with various expressions ===
fn main() {
    // Let bindings
    let x = 5
    let mut y = 10
    let z: i32 = 15
    
    // Arithmetic
    let sum = x + y + z
    let product = x * y
    let diff = y - x
    let quotient = z / x
    let remainder = z % x
    
    // Comparisons
    let is_greater = x > y
    let is_equal = x == 5
    let is_not_equal = x != y
    let is_less_or_equal = x <= y
    
    // Logical operators
    let both = is_greater && is_equal
    let either = is_greater || is_equal
    let negated = !is_greater
    
    // Assignment
    y = y + 1
    y = x * 2
    
    // Struct creation
    let p1 = Point { x: 1, y: 2 }
    let p2 = Point { x: 3, y: 4 }
    
    // Nested struct
    let rect = Rectangle {
        origin: Point { x: 0, y: 0 },
        width: 100,
        height: 50,
    }
    
    // Field access
    let px = p1.x
    let rect_width = rect.width
    let origin_x = rect.origin.x
    
    // Method calls (once impl is supported)
    // let p3 = Point.new(5, 6)
    // let dist = p1.distance(&p2)
    
    // Function calls
    let fib10 = fibonacci(10)
    let fact5 = factorial(5)
    
    // References (immutable)
    let ref_x = &x
    
    // Dereference
    let deref_x = *ref_x
    
    // If expressions
    let max = if x > y {
        x
    } else {
        y
    }
    
    // Nested if
    let grade = if sum > 90 {
        5
    } else {
        if sum > 80 {
            4
        } else {
            3
        }
    }
    
    // While loops
    let mut counter = 0
    while counter < 10 {
        counter = counter + 1
    }
    
    // Block expressions
    let block_result = {
        let a = 1
        let b = 2
        a + b
    }
    
    // Unary operators
    let neg = -x
    let not_true = !true
    
    // Chained comparisons (parsed as binary)
    let in_range = x > 0 && x < 100
}

