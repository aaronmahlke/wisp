import std.io.print

pub enum TestOption<T> {
    TSome(T),
    TNone
}

fn main() {
    let x: TestOption<i32> = TSome(42)
    print(&42)
}

