import std.io

// Define a trait
trait Printable {
    fn print_value(&self)
}

struct Point {
    x: i32,
    y: i32,
}

struct Rectangle {
    width: i32,
    height: i32,
}

// Implement trait for Point
impl Printable for Point {
    fn print_value(&self) {
        print("Point(");
        print_i32(self.x);
        print(", ");
        print_i32(self.y);
        print(")");
    }
}

// Implement trait for Rectangle
impl Printable for Rectangle {
    fn print_value(&self) {
        print("Rect(");
        print_i32(self.width);
        print("x");
        print_i32(self.height);
        print(")");
    }
}

fn main() -> i32 {
    let p = Point { x: 3, y: 4 };
    let r = Rectangle { width: 10, height: 5 };
    
    print("Point: ");
    p.print_value();
    println();
    
    print("Rectangle: ");
    r.print_value();
    println();
    
    0
}

