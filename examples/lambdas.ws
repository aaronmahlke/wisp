import std.io.{ print }

fn main() {
    // Lambda with type annotations
    let add = (x: i32, y: i32) -> x + y;
    
    // Call the lambda
    let result = add(10, 20);
    print(&"10 + 20 = {result}");
    
    // Lambda with single parameter
    let square = (x: i32) -> x * x;
    let sq = square(5);
    print(&"5^2 = {sq}");
    
    // Lambda returning a comparison
    let is_positive = (n: i32) -> n > 0;
    let pos1 = is_positive(42);
    print(&"42 > 0? {pos1}");
    let pos2 = is_positive(-5);
    print(&"-5 > 0? {pos2}");
    
    // Lambda with block body
    let complex = (a: i32, b: i32) -> {
        let sum = a + b;
        let product = a * b;
        sum + product
    };
    let c = complex(3, 4);
    print(&"(3+4) + (3*4) = {c}");
}
