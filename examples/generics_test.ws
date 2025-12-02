import "../std/io"

// Generic identity function
fn identity<T>(x: T) -> T {
    x
}

// Generic function with two type parameters
fn first<T, U>(a: T, b: U) -> T {
    a
}

fn second<T, U>(a: T, b: U) -> U {
    b
}

// Generic struct
struct Pair<T> {
    first: T,
    second: T,
}

fn print_result(label: str, value: i32) {
    print(&label);
    print(&value);
    println();
}

fn print_line(s: str) {
    print(&s);
    println();
}

fn main() -> i32 {
    print_line("=== Generics Test ===");
    
    // Test identity with different types
    let x = identity(42);
    print_result("identity(42) = ", x);
    
    let y = identity(100);
    print_result("identity(100) = ", y);
    
    // Test first/second with different types
    let a = first(10, 20);
    print_result("first(10, 20) = ", a);
    
    let b = second(10, 20);
    print_result("second(10, 20) = ", b);
    
    // Multiple instantiations of the same generic
    let c = identity(identity(5));
    print_result("identity(identity(5)) = ", c);
    
    println();
    print_line("=== All generics tests passed! ===");
    0
}
