import "../std/io"

// Define a trait
trait Printable {
    fn print_value(&self)
}

struct Point {
    x: i32,
    y: i32,
}

struct Number {
    value: i32,
}

// Implement Printable for Point
impl Printable for Point {
    fn print_value(&self) {
        print("Point(");
        print_i32(self.x);
        print(", ");
        print_i32(self.y);
        print(")");
    }
}

// Implement Printable for Number
impl Printable for Number {
    fn print_value(&self) {
        print("Number(");
        print_i32(self.value);
        print(")");
    }
}

// Generic function with trait bound
fn print_twice<T: Printable>(x: &T) {
    x.print_value();
    print(" and ");
    x.print_value();
    println();
}

fn main() -> i32 {
    print_line("=== Trait Bounds Test ===");

    let p = Point { x: 3, y: 4 };
    let n = Number { value: 42 };

    print("Printing Point twice: ");
    print_twice(&p);

    print("Printing Number twice: ");
    print_twice(&n);

    print_line("=== Done! ===");
    0
}
