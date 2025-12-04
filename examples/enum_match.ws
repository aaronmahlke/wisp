import std.io.print

pub enum TestOption<T> {
    TSome(T),
    TNone
}

fn main() {
    let x: TestOption<i32> = TSome(42)
    
    match x {
        TSome(v) -> print(&v),
        TNone -> print(&0),
    }
}

