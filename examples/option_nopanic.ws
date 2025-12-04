import std.io.print

pub enum TestOption<T> {
    TSome(T),
    TNone
}

impl<T> TestOption<T> {
    fn or(self, default: T) -> T {
        match self {
            TSome(v) -> v,
            TNone -> default,
        }
    }
}

fn main() {
    let x: TestOption<i32> = TSome(42)
    let val = x.or(0)
    print(&val)
}
